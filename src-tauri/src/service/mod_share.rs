//! Copy mods between a server and a launcher instance on this PC, so a client
//! can join a modded server (or a tested instance can become one).
//!
//! The jars themselves are copied, not looked up again: both sides must be on
//! the same game version and loader, and the player then runs exactly what the
//! server runs. Mods that cannot run on the receiving side stay behind, and a
//! jar of the same project there is replaced so two versions never load.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::minecraft::modrinth::{InstalledAddonEntry, Modrinth, ModrinthStatus};
use crate::launcher::{content, Profile};
use crate::service::InstanceService;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModShareDirection {
    ToInstance,
    ToServer,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModShareReport {
    pub copied: Vec<String>,
    pub already: Vec<String>,
    pub skipped: Vec<SkippedMod>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedMod {
    pub file: String,
    /// `disabled`, `wrong_side` or `failed`.
    pub reason: String,
}

#[derive(Debug, PartialEq, Eq)]
enum Step {
    Copy { file: String, replaces: Option<String> },
    Already(String),
    Skip(String, &'static str),
}

/// Whether a jar can run on the receiving side. Modrinth says for the mods it
/// knows; a Fabric jar it does not know says so itself.
fn runs_there(entry: &InstalledAddonEntry, direction: ModShareDirection, dir: &Path) -> bool {
    let unsupported = |side: &Option<String>| side.as_deref() == Some("unsupported");
    match (&entry.modrinth, direction) {
        (Some(found), ModShareDirection::ToInstance) => !unsupported(&found.project.client_side),
        (Some(found), ModShareDirection::ToServer) => !unsupported(&found.project.server_side),
        (None, ModShareDirection::ToInstance) => fabric_environment(&dir.join(&entry.file_name)).as_deref() != Some("server"),
        (None, ModShareDirection::ToServer) => fabric_environment(&dir.join(&entry.file_name)).as_deref() != Some("client"),
    }
}

fn fabric_environment(jar: &Path) -> Option<String> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(jar).ok()?).ok()?;
    let mut text = String::new();
    zip.by_name("fabric.mod.json").ok()?.take(1024 * 1024).read_to_string(&mut text).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json["environment"].as_str().map(String::from)
}

fn project_of(entry: &InstalledAddonEntry) -> Option<&str> {
    entry.modrinth.as_ref().map(|found| found.project.project_id.as_str())
}

/// Which jars go: those that run on the receiving side, and whatever those
/// require. Modrinth can mark a library as server-only while a mod that runs
/// on both sides needs it (Lithostitched for Tectonic), and the mod's own
/// requirement is the one the game enforces at start.
fn going(source: &[InstalledAddonEntry], runs: impl Fn(&InstalledAddonEntry) -> bool) -> HashSet<String> {
    let enabled = || source.iter().filter(|entry| entry.enabled);
    let mut going: HashSet<String> = enabled().filter(|entry| runs(entry)).map(|entry| entry.file_name.clone()).collect();
    loop {
        let required: HashSet<&str> = enabled()
            .filter(|entry| going.contains(&entry.file_name))
            .filter_map(|entry| entry.modrinth.as_ref())
            .flat_map(|found| &found.installed_version.dependencies)
            .filter(|dependency| dependency.dependency_type == "required")
            .filter_map(|dependency| dependency.project_id.as_deref())
            .collect();
        let added: Vec<String> = enabled()
            .filter(|entry| !going.contains(&entry.file_name) && project_of(entry).is_some_and(|id| required.contains(id)))
            .map(|entry| entry.file_name.clone())
            .collect();
        if added.is_empty() {
            return going;
        }
        going.extend(added);
    }
}

/// What to do with each jar. `runs` answers for the receiving side.
fn plan(source: &[InstalledAddonEntry], target: &[InstalledAddonEntry], runs: impl Fn(&InstalledAddonEntry) -> bool) -> Vec<Step> {
    let going = going(source, runs);
    source
        .iter()
        .map(|entry| {
            let file = entry.file_name.clone();
            if !entry.enabled {
                return Step::Skip(file, "disabled");
            }
            if !going.contains(&file) {
                return Step::Skip(file, "wrong_side");
            }
            let same_project = |other: &&InstalledAddonEntry| project_of(entry).is_some() && project_of(entry) == project_of(other);
            // The same jar, or one the user switched off there: left alone.
            if target.iter().any(|other| other.sha1 == entry.sha1) || target.iter().filter(|other| !other.enabled).any(|other| same_project(&other)) {
                return Step::Already(file);
            }
            let replaces = target
                .iter()
                .find(|other| same_project(other) || other.file_name == entry.file_name)
                .map(|other| other.file_name.clone());
            Step::Copy { file, replaces }
        })
        .collect()
}

/// Copy through a temporary name, so a jar is never half there.
fn copy_jar(from: &Path, to_dir: &Path, file: &str, replaces: Option<&str>) -> AppResult<()> {
    let target = to_dir.join(file);
    let temp = to_dir.join(format!("{file}.nebula-part"));
    std::fs::copy(from.join(file), &temp)?;
    if let Err(error) = std::fs::rename(&temp, &target) {
        let _ = std::fs::remove_file(&temp);
        return Err(error.into());
    }
    if let Some(old) = replaces.filter(|old| *old != file) {
        std::fs::remove_file(to_dir.join(old))?;
    }
    Ok(())
}

impl InstanceService {
    pub async fn share_mods(&self, instance_id: &str, profile: &Profile, direction: ModShareDirection) -> AppResult<ModShareReport> {
        self.ensure_not_transferring(instance_id).await?;
        let instance = self.store.get(instance_id).await?;
        let (server_dir, mut addon_profile) = self.addon_profile(&instance)?;
        let loader = content::loader_tag(&profile.loader);
        if addon_profile.project_type != "mod"
            || addon_profile.game_version != profile.game_version
            || !loader.is_some_and(|tag| addon_profile.loaders.iter().any(|known| known == tag))
        {
            return Err(AppError::new(ErrorCode::InvalidInput)
                .with("reason", "mods_mismatch")
                .detail("The server and the instance must use the same Minecraft version and mod loader."));
        }
        if direction == ModShareDirection::ToServer && self.addons_locked(instance_id).await {
            return Err(AppError::new(ErrorCode::InstanceAlreadyRunning).with("instance", instance_id));
        }

        let instance_dir = content::mods_dir(profile)?;
        std::fs::create_dir_all(&instance_dir)?;
        std::fs::create_dir_all(&server_dir)?;
        addon_profile.loaders.truncate(1);
        let client = Modrinth::new();
        let server = client.identify_dir(&server_dir, &addon_profile).await?;
        let local = client.identify_dir(&instance_dir, &addon_profile).await?;
        // Without Modrinth there is no telling which side a mod belongs on,
        // and a client-only mod stops a server from starting.
        if server.modrinth_status != ModrinthStatus::Ok || local.modrinth_status != ModrinthStatus::Ok {
            return Err(AppError::new(ErrorCode::UpstreamUnavailable).with("service", "modrinth"));
        }

        let (from, to, source, target) = match direction {
            ModShareDirection::ToInstance => (server_dir, instance_dir, server.addons, local.addons),
            ModShareDirection::ToServer => (instance_dir, server_dir, local.addons, server.addons),
        };
        let projects: HashMap<String, String> = source
            .iter()
            .filter_map(|entry| project_of(entry).map(|id| (entry.file_name.clone(), id.to_string())))
            .collect();

        let report = tokio::task::spawn_blocking(move || {
            let mut report = ModShareReport::default();
            for step in plan(&source, &target, |entry| runs_there(entry, direction, &from)) {
                match step {
                    Step::Already(file) => report.already.push(file),
                    Step::Skip(file, reason) => report.skipped.push(SkippedMod { file, reason: reason.to_string() }),
                    Step::Copy { file, replaces } => match copy_jar(&from, &to, &file, replaces.as_deref()) {
                        Ok(()) => report.copied.push(file),
                        // A running game holds its jars; the rest still go.
                        Err(_) => report.skipped.push(SkippedMod { file, reason: "failed".to_string() }),
                    },
                }
            }
            report
        })
        .await
        .map_err(AppError::unexpected)?;

        match direction {
            ModShareDirection::ToInstance => {
                let sources: Vec<(String, String)> = report
                    .copied
                    .iter()
                    .filter_map(|file| projects.get(file).map(|id| (file.clone(), id.clone())))
                    .collect();
                content::record_sources(profile, &sources)?;
            }
            ModShareDirection::ToServer => self.after_addon_change(instance_id).await,
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::minecraft::modrinth::{AddonProjectSummary, AddonVersion, ModrinthMatch};

    fn jar(file: &str, sha1: &str, enabled: bool, project: Option<&str>) -> InstalledAddonEntry {
        let modrinth = project.map(|id| {
            let raw = serde_json::json!({
                "id": "v", "project_id": id, "name": "v", "version_number": "1", "version_type": "release",
                "date_published": "2026-01-01T00:00:00Z", "game_versions": ["26.3"], "loaders": ["fabric"],
                "files": [{ "url": "https://cdn.modrinth.com/a.jar", "filename": "a.jar", "primary": true, "size": 1, "hashes": { "sha1": sha1 } }],
                "dependencies": [],
            });
            let profile = crate::game::minecraft::modrinth::AddonProfile {
                project_type: "mod".into(),
                loaders: vec!["fabric".into()],
                game_version: "26.3".into(),
            };
            let installed_version: AddonVersion = crate::game::minecraft::modrinth::parse_version(&raw, &profile).expect("version");
            ModrinthMatch {
                project: AddonProjectSummary {
                    project_id: id.into(),
                    slug: id.into(),
                    title: id.into(),
                    description: String::new(),
                    icon_url: None,
                    project_type: "mod".into(),
                    client_side: None,
                    server_side: None,
                },
                installed_version,
                latest_version: None,
                update_available: false,
            }
        });
        InstalledAddonEntry {
            file_name: file.into(),
            display_name: file.into(),
            enabled,
            size_bytes: 1,
            modified: None,
            sha1: sha1.into(),
            modrinth,
        }
    }

    #[test]
    fn copies_new_jars_and_leaves_the_same_jar_alone() {
        let steps = plan(&[jar("a.jar", "h1", true, Some("a")), jar("b.jar", "h2", true, None)], &[jar("other.jar", "h2", true, None)], |_| true);
        assert_eq!(steps[0], Step::Copy { file: "a.jar".into(), replaces: None });
        assert_eq!(steps[1], Step::Already("b.jar".into()));
    }

    #[test]
    fn another_version_of_a_project_is_replaced_so_two_never_load() {
        let steps = plan(&[jar("a-2.jar", "new", true, Some("a"))], &[jar("a-1.jar", "old", true, Some("a"))], |_| true);
        assert_eq!(steps[0], Step::Copy { file: "a-2.jar".into(), replaces: Some("a-1.jar".into()) });
    }

    #[test]
    fn a_mod_switched_off_on_the_other_side_stays_off() {
        let steps = plan(&[jar("a-2.jar", "new", true, Some("a"))], &[jar("a-1.jar.disabled", "old", false, Some("a"))], |_| true);
        assert_eq!(steps[0], Step::Already("a-2.jar".into()));
    }

    #[test]
    fn disabled_and_wrong_side_mods_stay_behind() {
        let source = [jar("off.jar.disabled", "h1", false, None), jar("server.jar", "h2", true, None)];
        let steps = plan(&source, &[], |entry| entry.file_name != "server.jar");
        assert_eq!(steps[0], Step::Skip("off.jar.disabled".into(), "disabled"));
        assert_eq!(steps[1], Step::Skip("server.jar".into(), "wrong_side"));
    }

    #[test]
    fn a_library_a_going_mod_requires_goes_too() {
        let mut needs = jar("tectonic.jar", "h1", true, Some("tectonic"));
        needs.modrinth.as_mut().expect("known").installed_version.dependencies = serde_json::from_value(
            serde_json::json!([{ "project_id": "litho", "dependency_type": "required" }]),
        )
        .expect("dependencies");
        let source = [needs, jar("litho.jar", "h2", true, Some("litho")), jar("server.jar", "h3", true, Some("server"))];
        let steps = plan(&source, &[], |entry| entry.file_name == "tectonic.jar");
        assert_eq!(steps[1], Step::Copy { file: "litho.jar".into(), replaces: None });
        assert_eq!(steps[2], Step::Skip("server.jar".into(), "wrong_side"));
    }

    #[test]
    fn unknown_jars_never_count_as_the_same_project() {
        let steps = plan(&[jar("a.jar", "h1", true, None)], &[jar("b.jar", "h2", true, None)], |_| true);
        assert_eq!(steps[0], Step::Copy { file: "a.jar".into(), replaces: None });
    }
}

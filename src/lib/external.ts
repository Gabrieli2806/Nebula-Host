/**
 * Opening a web link in the user's browser.
 *
 * Inside the app `window.open` does nothing (the webview has no tabs), so
 * every external link goes through the opener plugin. Its URL scope is
 * granted in `src-tauri/capabilities/default.json` (`opener:allow-default-urls`:
 * http, https, mailto, tel); without it the call is refused silently.
 */

export async function openExternal(url: string): Promise<boolean> {
  try {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
    return true;
  } catch (error) {
    // Outside Tauri (the browser preview) there is a real window to open.
    if (!("__TAURI_INTERNALS__" in window)) {
      window.open(url, "_blank", "noopener");
      return true;
    }
    console.warn("could not open link", url, error);
    return false;
  }
}

/**
 * The server's icon: the picture the game shows next to it in the
 * multiplayer list, which is also its tile on the card. Any image goes in;
 * the backend squares it (crop or fit) and writes the game's 64×64 PNG.
 */

import { useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { keys } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { api } from "@/lib/ipc";
import type { InstanceView } from "@/types/api";
import { Button, ErrorNotice, Icon, Panel, Segmented, ServerTile } from "@/ui";

type Fit = "cover" | "contain";

/**
 * Pictures above this are shrunk in the webview before they are sent. The
 * remote API takes JSON bodies of about 2 MB, and a phone photo as base64 is
 * far past that; the icon ends up 64 pixels wide either way.
 */
const SEND_AS_IS_BYTES = 1024 * 1024;
const PRESCALE_SIDE = 512;

export function ServerIconCard({ instance, accent }: { instance: InstanceView; accent?: string | null }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const input = useRef<HTMLInputElement>(null);
  const [fit, setFit] = useState<Fit>("cover");
  // The last picture picked, so switching crop/fit re-renders it without
  // asking for the file again.
  const [picked, setPicked] = useState<string | null>(null);

  const refresh = () => client.invalidateQueries({ queryKey: keys.instances });

  const save = useMutation({
    mutationFn: ({ data, fit }: { data: string; fit: Fit }) => api.instances.setIcon(instance.id, data, fit),
    onSuccess: refresh,
  });
  const clear = useMutation({
    mutationFn: () => api.instances.clearIcon(instance.id),
    onSuccess: () => {
      setPicked(null);
      return refresh();
    },
  });
  const busy = save.isPending || clear.isPending;

  const onFile = async (file: File | undefined) => {
    if (!file) return;
    save.reset();
    clear.reset();
    // A format the webview cannot draw goes as it is: the backend decodes
    // more than the browser does, and says so in its own words if it cannot.
    const data = await readImage(file).catch(() => readAsDataUrl(file).catch(() => ""));
    setPicked(data);
    save.mutate({ data, fit });
  };

  const changeFit = (next: Fit) => {
    setFit(next);
    if (picked) save.mutate({ data: picked, fit: next });
  };

  const hasIcon = Boolean(instance.icon_url);

  return (
    <Panel title={t("serverIcon.title")}>
      <div className="server-icon-card">
        <div className="server-icon-preview" aria-hidden="true">
          <ServerTile name={instance.name} identity={accent} image={instance.icon_url} size="lg" lit />
        </div>
        <div className="server-icon-body">
          <p className="pref-help">{t("serverIcon.help")}</p>
          {!hasIcon ? <p className="pref-hint">{t("serverIcon.none")}</p> : null}
          <div className="server-icon-actions">
            <Button icon="image" disabled={busy} onClick={() => input.current?.click()}>
              {save.isPending ? t("serverIcon.working") : hasIcon ? t("serverIcon.change") : t("serverIcon.choose")}
            </Button>
            {hasIcon ? (
              <Button variant="quiet" icon="trash" disabled={busy} onClick={() => clear.mutate()}>
                {t("serverIcon.remove")}
              </Button>
            ) : null}
            <Segmented<Fit>
              label={t("serverIcon.fitLabel")}
              size="sm"
              value={fit}
              onChange={changeFit}
              options={[
                { id: "cover", label: t("serverIcon.fitCover") },
                { id: "contain", label: t("serverIcon.fitContain") },
              ]}
            />
          </div>
          {hasIcon && instance.status === "online" ? (
            <p className="pref-hint">
              <Icon name="restart" size={12} />
              {t("serverIcon.restartHint")}
            </p>
          ) : null}
          <input
            ref={input}
            type="file"
            accept="image/png,image/jpeg,image/webp,image/gif,image/bmp,image/x-icon,.png,.jpg,.jpeg,.webp,.gif,.bmp,.ico"
            className="sr-only"
            tabIndex={-1}
            aria-hidden="true"
            onChange={(event) => {
              void onFile(event.target.files?.[0]);
              // The same file picked twice in a row must still fire.
              event.target.value = "";
            }}
          />
        </div>
      </div>
      {save.error || clear.error ? (
        <div className="px-5 pb-4">
          <ErrorNotice error={save.error ?? clear.error} />
        </div>
      ) : null}
    </Panel>
  );
}

function readAsDataUrl(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

/**
 * The picture as a `data:` URL, shrunk to at most 512 px a side when the
 * file is large (see SEND_AS_IS_BYTES). Small files go as they are, so the
 * backend's own resize decides the quality.
 */
async function readImage(file: File): Promise<string> {
  if (file.size <= SEND_AS_IS_BYTES) return readAsDataUrl(file);
  const bitmap = await createImageBitmap(file);
  try {
    const scale = Math.min(1, PRESCALE_SIDE / Math.max(bitmap.width, bitmap.height));
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.round(bitmap.width * scale));
    canvas.height = Math.max(1, Math.round(bitmap.height * scale));
    const context = canvas.getContext("2d");
    if (!context) throw new Error("no 2d context");
    context.imageSmoothingQuality = "high";
    context.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
    return canvas.toDataURL("image/png");
  } finally {
    bitmap.close();
  }
}

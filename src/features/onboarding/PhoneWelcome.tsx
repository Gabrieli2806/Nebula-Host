/**
 * The phone app's first screen. Its main job is managing the servers on your
 * computer (or a Raspberry Pi) from the phone; hosting on the phone itself is
 * offered too, as the second choice, with what it costs said plainly.
 */

import { useState } from "react";

import { PairDialog } from "@/app/NodeSwitcher";
import { TitleBar } from "@/app/TitleBar";
import { useI18n } from "@/i18n";
import { Icon, Mark } from "@/ui";

import "./PhoneWelcome.css";

export const PHONE_HOSTING_KEY = "nebula.phoneHosting";

export function PhoneWelcome({ onHostHere }: { onHostHere: () => void }) {
  const { t } = useI18n();
  const [pairing, setPairing] = useState(false);

  return (
    <div className="flex h-full flex-col">
      <TitleBar />
      <main className="phone-welcome">
        <Mark size={44} />
        <h1 className="display phone-welcome-title">{t("phoneWelcome.title")}</h1>
        <p className="phone-welcome-intro">{t("phoneWelcome.intro")}</p>

        <button type="button" className="phone-welcome-choice" data-primary="" onClick={() => setPairing(true)}>
          <Icon name="link" size={22} />
          <span className="flex min-w-0 flex-col items-start gap-1">
            <span className="phone-welcome-choice-title">{t("phoneWelcome.connect")}</span>
            <span className="phone-welcome-choice-hint">{t("phoneWelcome.connectHint")}</span>
          </span>
        </button>

        <button
          type="button"
          className="phone-welcome-choice"
          onClick={() => {
            try {
              localStorage.setItem(PHONE_HOSTING_KEY, "1");
            } catch {
              // Storage blocked: the choice lasts for this session.
            }
            onHostHere();
          }}
        >
          <Icon name="servers" size={22} />
          <span className="flex min-w-0 flex-col items-start gap-1">
            <span className="phone-welcome-choice-title">{t("phoneWelcome.host")}</span>
            <span className="phone-welcome-choice-hint">{t("phoneWelcome.hostHint")}</span>
          </span>
        </button>

        <p className="phone-welcome-note">
          <Icon name="info" size={13} />
          <span>{t("phoneWelcome.note")}</span>
        </p>
      </main>
      {pairing ? <PairDialog open onClose={() => setPairing(false)} /> : null}
    </div>
  );
}

/** Said wherever a server is about to be hosted on the phone itself. */
export function PhoneHostingNotice() {
  const { t } = useI18n();
  return (
    <p className="phone-hosting-notice">
      <Icon name="warning" size={14} />
      <span>{t("phoneWelcome.hostWarning")}</span>
    </p>
  );
}

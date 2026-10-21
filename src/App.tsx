import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { HashRouter, Navigate, Route, Routes } from "react-router-dom";

import { AccentSync } from "@/app/AccentSync";
import { AppShell } from "@/app/AppShell";
import { ThemeSync } from "@/app/ThemeSync";
import { FirstRun } from "@/features/onboarding/FirstRun";
import { AgreementGate } from "@/features/server/agreement/Agreement";
import { ServerScreen } from "@/features/server/ServerScreen";
import { ServersScreen } from "@/features/servers/ServersScreen";
import { SettingsScreen } from "@/features/settings/SettingsScreen";
import { MachinesScreen } from "@/features/machines/MachinesScreen";
import { LauncherScreen } from "@/features/launcher/LauncherScreen";
import { useSuppressNativeContextMenu } from "@/ui/ContextMenu";
import { errorActions } from "@/ui";
import { JavaInstallButton } from "@/features/java/JavaInstall";
import { I18nProvider } from "@/i18n";
import { WebPair } from "@/features/web/WebPair";
import { IS_WEB, webSession } from "@/lib/web";

// A missing Java says which one; offer to install it right there.
errorActions.java_not_found = (error) =>
  error.params.major ? <JavaInstallButton major={Number(error.params.major)} /> : null;

const client = new QueryClient({
  defaultOptions: {
    queries: {
      // Everything here is local IPC: cheap to call, and wrong answers are
      // worse than an extra call.
      staleTime: 2_000,
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
});

export function App() {
  useSuppressNativeContextMenu();
  return (
    <QueryClientProvider client={client}>
      <I18nProvider>
        <AccentSync />
        <ThemeSync />
        {/* The web panel signs in with a pairing code before anything else. */}
        {IS_WEB && !webSession() ? <WebPair /> : <Shell />}
      </I18nProvider>
    </QueryClientProvider>
  );
}

function Shell() {
  return (
    <>
        <AgreementGate />
        <HashRouter>
          <Routes>
            <Route path="/welcome" element={<FirstRun />} />
            <Route element={<AppShell />}>
              <Route path="/servers" element={<ServersScreen />} />
              <Route path="/launcher" element={<LauncherScreen />} />
              {/* The open panel lives in the URL, so the sidebar can be plain
                  links and the back button walks through panels. */}
              <Route
                path="/servers/:instanceId"
                element={<Navigate to="overview" replace />}
              />
              <Route path="/servers/:instanceId/:panel" element={<ServerScreen />} />
              <Route path="/settings" element={<SettingsScreen />} />
              <Route path="/machines" element={<MachinesScreen />} />
            </Route>
            <Route path="*" element={<Navigate to="/servers" replace />} />
          </Routes>
        </HashRouter>
    </>
  );
}

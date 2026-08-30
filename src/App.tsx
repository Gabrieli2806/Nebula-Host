import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { HashRouter, Navigate, Route, Routes } from "react-router-dom";

import { AccentSync } from "@/app/AccentSync";
import { AppShell } from "@/app/AppShell";
import { FirstRun } from "@/features/onboarding/FirstRun";
import { ServerScreen } from "@/features/server/ServerScreen";
import { ServersScreen } from "@/features/servers/ServersScreen";
import { SettingsScreen } from "@/features/settings/SettingsScreen";
import { I18nProvider } from "@/i18n";

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
  return (
    <QueryClientProvider client={client}>
      <I18nProvider>
        <AccentSync />
        <HashRouter>
          <Routes>
            <Route path="/welcome" element={<FirstRun />} />
            <Route element={<AppShell />}>
              <Route path="/servers" element={<ServersScreen />} />
              {/* The open panel lives in the URL, so the sidebar can be plain
                  links and the back button walks through panels. */}
              <Route
                path="/servers/:instanceId"
                element={<Navigate to="overview" replace />}
              />
              <Route path="/servers/:instanceId/:panel" element={<ServerScreen />} />
              <Route path="/settings" element={<SettingsScreen />} />
            </Route>
            <Route path="*" element={<Navigate to="/servers" replace />} />
          </Routes>
        </HashRouter>
      </I18nProvider>
    </QueryClientProvider>
  );
}

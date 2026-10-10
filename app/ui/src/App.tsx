import { useState } from 'react';
import type { ClientSelection } from './client/createClient';
import { scenarioHasCompletedOnboarding } from './client/mockScenarios';
import { Button, EmptyState, Spinner, ThemeProvider, ToastProvider } from './design';
import { DebugView } from './features/DebugView';
import { DesignGallery } from './features/DesignGallery';
import { IncomingRequestSheet } from './features/devices/IncomingRequestSheet';
import { PairingSheet } from './features/devices/PairingSheet';
import { Onboarding } from './features/onboarding/Onboarding';
import { AppShell } from './features/shell/AppShell';
import { PermissionSheet } from './features/shell/PermissionSheet';
import { SwitchHud } from './features/shell/SwitchHud';
import { TrayView } from './features/tray/TrayView';
import { FlowProvider, useFlow } from './state/FlowProvider';
import { loadOnboardingComplete, saveOnboardingComplete } from './state/prefs';
import { UiProvider } from './state/ui';
import { useHashRoute } from './state/useHashRoute';

function Splash() {
  const s = useFlow();
  if (s.connection === 'closed') {
    return (
      <div className="fl-splash">
        <EmptyState
          title="Flow isn’t responding"
          actions={<Button onClick={() => window.location.reload()}>Reload</Button>}
        >
          {s.connectionError ?? 'Restart Flow if this keeps happening.'}
        </EmptyState>
      </div>
    );
  }
  return (
    <div className="fl-splash">
      <Spinner size={22} label="Connecting to Flow" />
    </div>
  );
}

function Routes({
  route,
  go,
  selection,
}: {
  route: string;
  go(p: string): void;
  selection: ClientSelection;
}) {
  const s = useFlow();
  const [onboarding, setOnboarding] = useState(() => {
    if (new URLSearchParams(window.location.search).get('onboarding') === '1') return true;
    if (loadOnboardingComplete()) return false;
    return selection.kind !== 'mock' || !scenarioHasCompletedOnboarding(selection.scenario ?? '');
  });

  if (route === '/design') return <DesignGallery />;
  if (route === '/debug') return <DebugView />;
  if (!s.loaded) return <Splash />;

  const isTray = route === '/tray';
  return (
    <UiProvider navigate={go}>
      {onboarding && !isTray ? (
        <Onboarding
          onFinish={() => {
            saveOnboardingComplete();
            setOnboarding(false);
            go('/home');
          }}
        />
      ) : isTray ? (
        <TrayView />
      ) : (
        <AppShell route={route} />
      )}
      {!onboarding && <IncomingRequestSheet />}
      {!onboarding && <PairingSheet />}
      <PermissionSheet />
      {!isTray && !onboarding && <SwitchHud />}
    </UiProvider>
  );
}

export function App({ selection }: { selection: ClientSelection }) {
  const [route, go] = useHashRoute();
  return (
    <ThemeProvider>
      <ToastProvider>
        <FlowProvider client={selection.client}>
          <Routes route={route} go={go} selection={selection} />
        </FlowProvider>
      </ToastProvider>
    </ThemeProvider>
  );
}

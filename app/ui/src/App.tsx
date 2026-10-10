import type { ClientSelection } from './client/createClient';
import { ToastProvider, ThemeProvider } from './design';
import { DebugView } from './features/DebugView';
import { DesignGallery } from './features/DesignGallery';
import { FlowProvider } from './state/FlowProvider';
import { useHashRoute } from './state/useHashRoute';

export function App({ selection }: { selection: ClientSelection }) {
  const [route] = useHashRoute();
  return (
    <ThemeProvider>
      <ToastProvider>
        <FlowProvider client={selection.client}>
          {route === '/design' ? <DesignGallery /> : <DebugView />}
        </FlowProvider>
      </ToastProvider>
    </ThemeProvider>
  );
}

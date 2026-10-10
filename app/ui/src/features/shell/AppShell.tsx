import { Activity, Home, Keyboard, Laptop, Settings } from 'lucide-react';
import { useEffect, type ReactNode } from 'react';
import { Sidebar, SidebarItem, StatusDot, Toolbar, type DotTone } from '@flow/design-system';
import { useFlow } from '../../state/FlowProvider';
import { effectiveLink } from '../../state/selectors';
import { useUi } from '../../state/ui';
import { DevicesToolbarAction, DevicesView } from '../devices/DevicesView';
import { DiagnosticsView } from '../diagnostics/DiagnosticsView';
import { GeneralView } from '../general/GeneralView';
import { HomeView } from '../home/HomeView';
import { ShortcutsView } from '../shortcuts/ShortcutsView';
import { LinkBanner } from './LinkBanner';

const ICON = { size: 16, strokeWidth: 1.7 } as const;

interface Section {
  path: string;
  title: string;
  icon: ReactNode;
  view: ReactNode;
  action?: ReactNode;
}

const SECTIONS: Section[] = [
  { path: '/home', title: 'Home', icon: <Home {...ICON} />, view: <HomeView /> },
  {
    path: '/devices',
    title: 'Computers',
    icon: <Laptop {...ICON} />,
    view: <DevicesView />,
    action: <DevicesToolbarAction />,
  },
  {
    path: '/shortcuts',
    title: 'Keyboard & Shortcuts',
    icon: <Keyboard {...ICON} />,
    view: <ShortcutsView />,
  },
  { path: '/general', title: 'General', icon: <Settings {...ICON} />, view: <GeneralView /> },
  { path: '/diagnostics', title: 'Diagnostics', icon: <Activity {...ICON} />, view: <DiagnosticsView /> },
];

const LINK: Record<string, [string, DotTone, boolean]> = {
  connected: ['Connected', 'green', false],
  connecting: ['Connecting…', 'orange', true],
  reconnecting: ['Reconnecting…', 'orange', true],
  disconnected: ['Not connected', 'gray', false],
  no_computers: ['No computers yet', 'gray', false],
  error: ['Needs attention', 'red', false],
  permission_required: ['Needs permission', 'red', false],
};

/** Main window: System-Settings-style sidebar + content with link banners. */
export function AppShell({ route }: { route: string }) {
  const { navigate } = useUi();
  const s = useFlow();
  const current = SECTIONS.find((x) => x.path === route) ?? SECTIONS[0]!;
  const [text, tone, pulse] =
    s.connection === 'closed'
      ? (['Not responding', 'red', false] as const)
      : (LINK[effectiveLink(s)] ?? LINK.connected!);

  // ⌘1…⌘5 / Ctrl+1…5 jump between sections.
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
      const i = Number(e.key) - 1;
      if (i >= 0 && i < SECTIONS.length) {
        e.preventDefault();
        navigate(SECTIONS[i]!.path);
      }
    };
    window.addEventListener('keydown', on);
    return () => window.removeEventListener('keydown', on);
  }, [navigate]);

  return (
    <div className="fl-app">
      <Sidebar
        footer={
          <div className="fl-sidebar__footer" role="status" aria-label={`Connection: ${text}`}>
            <StatusDot tone={tone} pulse={pulse} />
            <span className="fl-sidebar__footer-text">{text}</span>
          </div>
        }
      >
        {SECTIONS.map((x, i) => (
          <SidebarItem
            key={x.path}
            icon={x.icon}
            label={x.title}
            active={x.path === current.path}
            shortcut={`Meta+${i + 1}`}
            onClick={() => navigate(x.path)}
          />
        ))}
      </Sidebar>
      <main className="fl-main" aria-labelledby="page-title">
        <Toolbar title={current.title} trailing={current.action} />
        <div className="fl-banner-region" data-testid="banner-region">
          <LinkBanner />
        </div>
        <div className="fl-content">
          <div className="fl-content__inner" key={current.path}>
            {current.view}
          </div>
        </div>
      </main>
    </div>
  );
}

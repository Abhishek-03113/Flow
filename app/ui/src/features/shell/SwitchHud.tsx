import { Keyboard, MonitorCheck } from 'lucide-react';
import { useEffect, useState } from 'react';
import { Hud } from '@flow/design-system';
import { useClient } from '../../state/FlowProvider';
import { deviceById, localDevice } from '../../state/selectors';

interface Shown {
  title: string;
  subtitle?: string;
  remote: boolean;
  key: number;
}

/** Transient centred HUD whenever control moves (`control_state_changed` with a new active screen). */
export function SwitchHud() {
  const client = useClient();
  const [shown, setShown] = useState<Shown | null>(null);
  const [leaving, setLeaving] = useState(false);

  useEffect(() => {
    let n = 0;
    let t1: ReturnType<typeof setTimeout>;
    let t2: ReturnType<typeof setTimeout>;
    const off = client.onActiveScreenChanged((screen) => {
      const s = client.getSnapshot();
      const me = localDevice(s);
      let next: Omit<Shown, 'key'>;
      if (s.controlState?.role === 'controlled') {
        const ctl = deviceById(s, s.controlState.controllerDeviceId ?? '');
        next =
          screen === 'local'
            ? {
                title: `${ctl?.name ?? 'Another computer'} took control`,
                subtitle: 'Its keyboard and mouse now work here',
                remote: true,
              }
            : {
                title: `Back on ${me?.name ?? 'this computer'}`,
                subtitle: 'Your own keyboard and mouse',
                remote: false,
              };
      } else if (screen === 'local') {
        next = { title: `Back on ${me?.name ?? 'this computer'}`, remote: false };
      } else {
        next = {
          title: `Now controlling ${deviceById(s, screen)?.name ?? 'another computer'}`,
          remote: true,
        };
      }
      clearTimeout(t1);
      clearTimeout(t2);
      setLeaving(false);
      setShown({ ...next, key: ++n });
      t1 = setTimeout(() => setLeaving(true), 1500);
      t2 = setTimeout(() => setShown(null), 1900);
    });
    return () => {
      off();
      clearTimeout(t1);
      clearTimeout(t2);
    };
  }, [client]);

  if (!shown) return null;
  return (
    <Hud
      key={shown.key}
      leaving={leaving}
      icon={
        shown.remote ? <MonitorCheck size={32} strokeWidth={1.4} /> : <Keyboard size={32} strokeWidth={1.4} />
      }
      title={shown.title}
      subtitle={shown.subtitle}
    />
  );
}

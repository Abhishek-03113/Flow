import { ArrowLeftRight, Keyboard, Laptop, MonitorCheck, Undo2, Unplug } from 'lucide-react';
import type { ReactNode } from 'react';
import { cx, Button, EmptyState, KeyCombo } from '@flow/design-system';
import { useClient, useFlow } from '../../state/FlowProvider';
import { localDevice, localOs, remoteDevices, situation, switchTargets } from '../../state/selectors';
import { useAction } from '../../state/useAction';
import { useUi } from '../../state/ui';

const ICON = { size: 30, strokeWidth: 1.5, 'aria-hidden': true } as const;

/** "Who controls what" — the single most important thing on screen. */
export function StatusCard() {
  const s = useFlow();
  const client = useClient();
  const { openPairing } = useUi();
  const { run, busy } = useAction();
  const sit = situation(s);
  const os = localOs(s);
  const me = localDevice(s);
  const targets = switchTargets(s);
  const key = s.settings?.switchKey.keys ?? [];
  const home = s.settings?.returnHomeKey?.keys ?? key;

  let title: string;
  let sub: ReactNode;
  let icon: ReactNode = <Keyboard {...ICON} />;
  let live = false;
  let actions: ReactNode = null;

  const switchButtons = targets
    .filter((d) => sit.kind !== 'controller-remote' || d.id !== sit.target?.id)
    .map((d) => (
      <Button
        key={d.id}
        icon={<ArrowLeftRight size={14} strokeWidth={1.8} />}
        disabled={busy}
        onClick={() => void run(() => client.switchTo(d.id))}
      >
        Switch to {d.name}
      </Button>
    ));

  switch (sit.kind) {
    case 'controller-home':
      title = `Your keyboard and mouse are on ${me?.name ?? 'this computer'}`;
      if (targets.length === 0 && remoteDevices(s).length > 0) {
        // Paired, but nothing reachable right now (offline, reconnecting, conflict).
        const others = remoteDevices(s);
        sub =
          others.length === 1
            ? `${others[0]!.name} isn’t connected right now. You can switch to it as soon as it’s back.`
            : 'Your other computers aren’t connected right now. You can switch as soon as one is back.';
        break;
      }
      if (targets.length === 0) {
        return (
          <div className="fl-card" data-testid="status-card">
            <EmptyState
              art={<Laptop size={44} strokeWidth={1.2} />}
              title="Add your first computer"
              actions={
                <Button variant="prominent" onClick={openPairing}>
                  Add Computer…
                </Button>
              }
            >
              Pair another computer to use this keyboard and mouse on it. It only takes a minute.
            </EmptyState>
          </div>
        );
      }
      sub = (
        <span>
          Press <KeyCombo keys={key} os={os} /> or move the pointer off the screen edge to move to another
          computer.
        </span>
      );
      actions = switchButtons;
      break;
    case 'controller-remote':
      live = true;
      icon = <MonitorCheck {...ICON} />;
      title = `Controlling ${sit.target?.name ?? 'another computer'}`;
      sub = (
        <span>
          Your keyboard and mouse are driving it. Press <KeyCombo keys={home} os={os} /> to come back.
        </span>
      );
      actions = (
        <>
          <Button
            variant="prominent"
            icon={<Undo2 size={14} strokeWidth={1.8} />}
            disabled={busy}
            onClick={() => void run(() => client.switchTo('local'))}
          >
            Return to this computer
          </Button>
          {switchButtons}
        </>
      );
      break;
    case 'controlled-driven':
      live = true;
      icon = <MonitorCheck {...ICON} />;
      title = `${sit.controller?.name ?? 'Another computer'} is controlling this computer`;
      sub =
        'Its keyboard and mouse are working here. You can take your own keyboard and mouse back at any time.';
      actions = (
        <Button
          variant="prominent"
          icon={<Undo2 size={14} strokeWidth={1.8} />}
          disabled={busy}
          onClick={() => void run(() => client.releaseControl())}
        >
          Release control
        </Button>
      );
      break;
    case 'controlled-waiting':
      title = 'This computer is ready';
      sub = `${sit.controller?.name ?? 'The controller'} will take over when you move to this screen. Until then your own keyboard and mouse work normally.`;
      break;
    case 'unknown':
      icon = <Unplug {...ICON} />;
      title = 'Can’t tell who’s in control right now';
      sub = 'Flow on this computer isn’t responding. This updates as soon as it’s back.';
      break;
    case 'controlled-unlinked':
      title = 'Waiting for a computer to control this one';
      sub = 'Add the computer whose keyboard and mouse you want to use here.';
      actions = (
        <Button variant="prominent" onClick={openPairing}>
          Add Computer…
        </Button>
      );
      break;
  }

  return (
    <div
      className={cx('fl-status', live && 'fl-status--live')}
      data-testid="status-card"
      role="region"
      aria-label="Control status"
    >
      <div className="fl-status__icon">{icon}</div>
      <div className="fl-status__main">
        <h2 className="t-title" data-testid="status-title">
          {title}
        </h2>
        <p className="fl-status__sub t-body">{sub}</p>
        {actions && <div className="fl-status__actions">{actions}</div>}
      </div>
    </div>
  );
}

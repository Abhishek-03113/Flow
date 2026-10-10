import { CircleAlert, ShieldAlert, Unplug, WifiOff } from 'lucide-react';
import { Banner, Button, useToast } from '@flow/design-system';
import { useClient, useFlow } from '../../state/FlowProvider';
import { useAction } from '../../state/useAction';
import { effectiveLink } from '../../state/selectors';
import { useUi } from '../../state/ui';

const I = { size: 18, strokeWidth: 1.7 } as const;

/** One banner per link state (data-model.md "DaemonLinkState"). Healthy states render nothing. */
export function LinkBanner() {
  const s = useFlow();
  const client = useClient();
  const toast = useToast();
  const { run, busy } = useAction();
  const { openPermission } = useUi();
  const peer = s.devices.find((d) => !d.isLocal)?.name ?? 'your other computer';
  const retry = (
    <Button size="small" loading={busy} onClick={() => void run(() => client.retryConnection())}>
      Try Again
    </Button>
  );

  if (s.connection === 'closed') {
    return (
      <Banner
        tone="red"
        role="alert"
        icon={<Unplug {...I} />}
        title="Flow isn’t responding"
        actions={
          <Button size="small" onClick={() => window.location.reload()}>
            Reload
          </Button>
        }
      >
        {s.connectionError ?? 'Restart Flow if this keeps happening.'}
      </Banner>
    );
  }
  // A role conflict is explained on the device itself; don't repeat it as a generic banner.
  const broken = s.devices.filter((d) => d.linkError);
  if (s.linkState === 'error' && broken.length > 0 && broken.every((d) => d.linkError === 'role_conflict'))
    return null;
  switch (s.linkState) {
    case 'reconnecting':
      return (
        <Banner
          tone="orange"
          icon={<WifiOff {...I} />}
          title="Reconnecting…"
          actions={
            <Button
              size="small"
              onClick={() =>
                void run(async () => {
                  await client.cancelReconnect();
                  toast.show('Stopped trying. Select Try Again whenever you want to reconnect.');
                })
              }
            >
              Stop Trying
            </Button>
          }
        >
          Lost the connection to {peer}. Flow is trying again.
        </Banner>
      );
    case 'disconnected':
      if (effectiveLink(s) === 'no_computers') return null;
      return (
        <Banner tone="gray" icon={<WifiOff {...I} />} title="Not connected" actions={retry}>
          Flow can’t reach {peer} right now.
        </Banner>
      );
    case 'error':
      return (
        <Banner
          tone="red"
          role="alert"
          icon={<CircleAlert {...I} />}
          title="Sharing is paused"
          actions={retry}
        >
          Something went wrong with the connection. Check {peer} and try again.
        </Banner>
      );
    case 'permission_required':
      return (
        <Banner
          tone="red"
          role="alert"
          icon={<ShieldAlert {...I} />}
          title="Flow needs permission"
          actions={
            <Button size="small" variant="prominent" onClick={openPermission}>
              Allow…
            </Button>
          }
        >
          Allow access to your keyboard and mouse so Flow can share them.
        </Banner>
      );
    default:
      return null;
  }
}

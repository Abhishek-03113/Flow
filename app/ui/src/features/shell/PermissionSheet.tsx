import { Button, Sheet } from '@flow/design-system';
import { useClient, useFlow } from '../../state/FlowProvider';
import { localOs } from '../../state/selectors';
import { useAction } from '../../state/useAction';
import { useUi } from '../../state/ui';
import { PermissionGuide } from '../permissions/PermissionGuide';

export function PermissionSheet() {
  const { permissionOpen, closePermission } = useUi();
  const s = useFlow();
  const client = useClient();
  const { run, busy } = useAction();
  return (
    <Sheet
      open={permissionOpen}
      title="Allow Flow to share your keyboard and mouse"
      onClose={closePermission}
      width={520}
      data-testid="permission-sheet"
      footer={
        <Button variant="prominent" onClick={closePermission}>
          {s.permission?.granted ? 'Done' : 'Not Now'}
        </Button>
      }
    >
      <PermissionGuide
        os={localOs(s)}
        permission={s.permission}
        requesting={busy}
        onRequest={() => void run(() => client.requestPermission())}
        onOpenSettings={(n) => void run(() => client.openPermissionSettings(n))}
        onRecheck={() => void run(() => client.recheckPermissions())}
        onRestart={() => void run(() => client.restartApp())}
      />
    </Sheet>
  );
}

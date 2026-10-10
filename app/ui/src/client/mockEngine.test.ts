import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { FlowClient } from './flowClient';
import { isFlowCommandError } from './errors';
import { MockTransport } from './mockTransport';

function setup(scenario: string, extra: { timeScale?: number } = {}) {
  const transport = new MockTransport({ scenario, ticks: false, latencyMs: 0, timeScale: 1, ...extra });
  const client = new FlowClient(transport);
  client.connect();
  return { client, transport, engine: transport.engine };
}
const flush = async () => {
  await vi.advanceTimersByTimeAsync(5);
};

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe('MockEngine', () => {
  it('sends the full snapshot on connect so the client is loaded', async () => {
    const { client } = setup('two-devices');
    await flush();
    const s = client.getSnapshot();
    expect(s.loaded).toBe(true);
    expect(s.devices.map((d) => d.id)).toEqual(['d1', 'd2']);
    expect(s.controlState).toEqual({
      role: 'controller',
      activeScreen: 'local',
      controllerDeviceId: null,
      entered: false,
    });
    expect(s.screenLayout.screens).toHaveLength(2);
    expect(s.diagnostics?.switchLatencyMsP95).toBeGreaterThan(0);
  });

  it('switch_to moves the active screen, updates device state and notifies the HUD listener', async () => {
    const { client } = setup('two-devices');
    await flush();
    const seen: string[] = [];
    client.onActiveScreenChanged((id) => seen.push(id));
    await client.switchTo('d2');
    expect(client.getSnapshot().controlState?.activeScreen).toBe('d2');
    expect(client.getSnapshot().devices.find((d) => d.id === 'd2')?.state).toBe('active');
    expect(client.getSnapshot().devices.find((d) => d.id === 'd1')?.state).toBe('inactive');
    await client.switchTo('local');
    expect(seen).toEqual(['d2', 'local']);
  });

  it('rejects unknown, unreachable and non-controller switches with contract error codes', async () => {
    const a = setup('two-devices');
    await flush();
    await expect(a.client.switchTo('nope')).rejects.toMatchObject({ code: 'device_not_found' });
    a.transport.engine.simulate.deviceState('d2', 'disconnected');
    await expect(a.client.switchTo('d2')).rejects.toMatchObject({ code: 'device_not_switchable' });

    const b = setup('controlled');
    await flush();
    await expect(b.client.switchTo('d2')).rejects.toMatchObject({ code: 'not_controller' });
  });

  it("the controlled scenario carries the controller's own arrangement", async () => {
    const { client } = setup('controlled');
    await flush();
    const screens = client.getSnapshot().screenLayout.screens;
    const mine = screens.find((r) => r.deviceId === 'd1')!;
    const theirs = screens.find((r) => r.deviceId === 'd2')!;
    expect(theirs.y + theirs.height).toBe(mine.y);
  });

  it('release_control hands the screen back to the controller; only valid when controlled', async () => {
    const { client } = setup('controlled');
    await flush();
    expect(client.getSnapshot().controlState).toMatchObject({ activeScreen: 'local', entered: true });
    await client.releaseControl();
    // A controlled computer's active screen is always `local`; `entered` says who drives it.
    expect(client.getSnapshot().controlState).toMatchObject({ activeScreen: 'local', entered: false });
    const other = setup('two-devices');
    await flush();
    await expect(other.client.releaseControl()).rejects.toMatchObject({ code: 'not_controlled' });
  });

  it('role-conflict scenario surfaces link_error and set_role resolves it', async () => {
    const { client } = setup('role-conflict');
    await flush();
    expect(client.getSnapshot().devices.find((d) => d.id === 'd2')).toMatchObject({
      linkError: 'role_conflict',
      state: 'error',
    });
    await client.setRole('controlled');
    const d2 = client.getSnapshot().devices.find((d) => d.id === 'd2');
    expect(d2?.linkError).toBeNull();
    expect(client.getSnapshot().settings?.role).toBe('controlled');
    expect(client.getSnapshot().controlState?.role).toBe('controlled');
    expect(client.getSnapshot().linkState).toBe('connected');
  });

  it('a role change that conflicts with the only peer makes the link error, not reconnecting', async () => {
    const { client } = setup('two-devices');
    await flush();
    await client.setRole('controlled'); // d2 is controlled too
    expect(client.getSnapshot().devices.find((d) => d.id === 'd2')?.linkError).toBe('role_conflict');
    expect(client.getSnapshot().linkState).toBe('error');
  });

  it('remove_device protects this computer and cleans the layout', async () => {
    const { client } = setup('two-devices');
    await flush();
    await expect(client.removeDevice('d1')).rejects.toMatchObject({ code: 'device_not_removable' });
    await client.removeDevice('d2');
    expect(client.getSnapshot().devices).toHaveLength(1);
    expect(client.getSnapshot().screenLayout.screens.map((s) => s.deviceId)).toEqual(['d1']);
  });

  it('walks the pairing state machine searching -> found -> requesting -> paired -> idle', async () => {
    const { client } = setup('no-devices');
    await flush();
    await client.startPairing();
    expect(client.getSnapshot().pairingSession.stage).toBe('searching');
    await expect(client.startPairing()).rejects.toMatchObject({ code: 'pairing_in_progress' });
    await vi.advanceTimersByTimeAsync(1300);
    expect(client.getSnapshot().pairingSession.stage).toBe('found');
    expect(client.getSnapshot().pairingSession.candidates.length).toBeGreaterThan(0);
    await client.pairWithCandidate('cand-1');
    expect(client.getSnapshot().pairingSession).toMatchObject({
      stage: 'requesting',
      targetName: 'Studio Mac mini',
      code: '482 913',
    });
    await vi.advanceTimersByTimeAsync(1600);
    expect(client.getSnapshot().pairingSession.stage).toBe('paired');
    expect(
      client.getSnapshot().devices.some((d) => d.name === 'Studio Mac mini' && d.role === 'controlled'),
    ).toBe(true);
    await vi.advanceTimersByTimeAsync(1700);
    expect(client.getSnapshot().pairingSession.stage).toBe('idle');
  });

  it('a candidate that declines produces a failed session with a readable error', async () => {
    const { client } = setup('no-devices');
    await flush();
    await client.startPairing();
    await vi.advanceTimersByTimeAsync(1300);
    await client.pairWithCandidate('cand-3');
    await vi.advanceTimersByTimeAsync(1600);
    expect(client.getSnapshot().pairingSession).toMatchObject({
      stage: 'failed',
      error: expect.stringContaining('Dev Laptop'),
    });
  });

  it('answers incoming pairing requests and rejects stale ids', async () => {
    const { client, engine } = setup('two-devices');
    await flush();
    engine.simulate.incomingPairingRequest();
    expect(client.getSnapshot().incomingPairingRequest?.deviceName).toContain('Windows');
    await expect(client.respondToPairingRequest('stale', 'accept')).rejects.toMatchObject({
      code: 'pairing_request_not_found',
    });
    await client.respondToPairingRequest('ipr-9f2c1a', 'accept');
    expect(client.getSnapshot().incomingPairingRequest).toBeNull();
    expect(client.getSnapshot().devices).toHaveLength(3);
  });

  it('validates screen layouts (unknown device, overlap) and persists a valid one', async () => {
    const { client } = setup('two-devices');
    await flush();
    const [a, b] = client.getSnapshot().screenLayout.screens;
    await expect(client.setScreenLayout({ screens: [{ ...a!, deviceId: 'zz' }] })).rejects.toMatchObject({
      code: 'device_not_found',
    });
    await expect(client.setScreenLayout({ screens: [a!, { ...b!, x: 10 }] })).rejects.toMatchObject({
      code: 'invalid_layout',
    });
    await client.setScreenLayout({ screens: [a!, { ...b!, x: 0, y: a!.height }] });
    expect(client.getSnapshot().screenLayout.screens[1]).toMatchObject({ x: 0, y: a!.height });
  });

  it('validates the switch key and remap rules; update_settings merges', async () => {
    const { client } = setup('two-devices');
    await flush();
    await expect(client.setSwitchKey({ label: '', keys: [] })).rejects.toMatchObject({
      code: 'invalid_switch_key',
    });
    await client.setSwitchKey({ label: 'Ctrl + Shift + Space', keys: ['Ctrl', 'Shift', 'Space'] });
    expect(client.getSnapshot().settings?.switchKey.keys).toEqual(['Ctrl', 'Shift', 'Space']);
    await client.updateSettings({ edgeSwitching: false, edgeSwitchDelayMs: 150 });
    expect(client.getSnapshot().settings).toMatchObject({
      edgeSwitching: false,
      edgeSwitchDelayMs: 150,
      shareMouse: true,
    });
    await expect(client.setRemapRules([{ from: '', to: 'Ctrl', whenTargetOs: null }])).rejects.toMatchObject({
      code: 'invalid_remap_rule',
    });
    // Like the engine: a key becomes a key, a modifier a modifier.
    await expect(
      client.setRemapRules([{ from: 'CapsLock', to: 'Ctrl', whenTargetOs: 'macos' }]),
    ).rejects.toMatchObject({ code: 'invalid_remap_rule' });
    await client.setRemapRules([{ from: 'CapsLock', to: 'End', whenTargetOs: 'macos' }]);
    expect(client.getSnapshot().remapRules).toEqual([{ from: 'CapsLock', to: 'End', whenTargetOs: 'macos' }]);
  });

  it('retry_connection is only valid when disconnected/error and ends connected', async () => {
    const { client } = setup('disconnected');
    await flush();
    await client.retryConnection();
    expect(client.getSnapshot().linkState).toBe('connecting');
    await vi.advanceTimersByTimeAsync(1300);
    expect(client.getSnapshot().linkState).toBe('connected');
    await expect(client.retryConnection()).rejects.toMatchObject({ code: 'link_not_recoverable' });
  });

  it('permission flow: request, then simulated grant clears permission_required', async () => {
    const { client } = setup('permission-missing');
    await flush();
    expect(client.getSnapshot().linkState).toBe('permission_required');
    await client.requestPermission();
    await vi.advanceTimersByTimeAsync(2600);
    expect(client.getSnapshot().permission?.granted).toBe(true);
    expect(client.getSnapshot().linkState).toBe('connected');
    await expect(client.requestPermission()).rejects.toMatchObject({ code: 'permission_already_granted' });
  });

  it('unknown commands fail with unknown_command as a FlowCommandError', async () => {
    const { transport } = setup('two-devices');
    await flush();
    const e = await transport.send('bogus').catch((x: unknown) => x);
    expect(isFlowCommandError(e) && e.code).toBe('unknown_command');
  });
});

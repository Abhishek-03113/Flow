import { act, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it } from 'vitest';
import { friendlyError } from '../copy';
import { FlowCommandError } from '../client/errors';
import { renderApp } from './renderApp';

beforeEach(() => {
  localStorage.clear();
  localStorage.setItem('flow.onboarding_complete', '1');
});

describe('Home', () => {
  it('shows who controls what and returns home with one click', async () => {
    const user = userEvent.setup();
    const { client } = renderApp('controlling-remote');
    expect(await screen.findByTestId('status-title')).toHaveTextContent('Controlling Office PC');
    await user.click(screen.getByRole('button', { name: /Return to this computer/ }));
    await waitFor(() => expect(client.getSnapshot().controlState?.activeScreen).toBe('local'));
    expect(screen.getByTestId('status-title')).toHaveTextContent(
      'Your keyboard and mouse are on MacBook Pro',
    );
  });

  it('empty state invites adding the first computer and opens the pairing sheet', async () => {
    const user = userEvent.setup();
    renderApp('no-devices');
    await screen.findByText('Add your first computer');
    await user.click(screen.getByRole('button', { name: 'Add Computer…' }));
    expect(await screen.findByRole('dialog', { name: 'Add Computer' })).toBeInTheDocument();
    expect(await screen.findByTestId('pairing-found')).toBeInTheDocument();
  });

  it('with no paired computer the link is neutral: no error banner, no "null ms"', async () => {
    renderApp('no-devices');
    await screen.findByText('Add your first computer');
    expect(screen.queryByText('Not connected')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Try Again' })).not.toBeInTheDocument();
    expect(screen.getAllByText('No computers yet').length).toBeGreaterThan(0);
    expect(screen.getByText('Switch time').previousElementSibling).toHaveTextContent('—');
    expect(document.body).not.toHaveTextContent('null');
  });

  it('a paired computer that is unreachable still gets the Not connected banner', async () => {
    renderApp('disconnected');
    await screen.findByRole('button', { name: 'Try Again' });
    expect(screen.getAllByText('Not connected').length).toBeGreaterThan(1);
    expect(screen.getByRole('button', { name: 'Try Again' })).toBeInTheDocument();
  });

  it('controlled computers can release control', async () => {
    const user = userEvent.setup();
    const { client } = renderApp('controlled');
    expect(await screen.findByTestId('status-title')).toHaveTextContent(
      'Desk PC is controlling this computer',
    );
    await user.click(screen.getByRole('button', { name: 'Release control' }));
    await waitFor(() => expect(client.getSnapshot().controlState?.entered).toBe(false));
    expect(screen.getByTestId('status-title')).toHaveTextContent('This computer is ready');
  });

  it('moves a screen tile with the keyboard and persists the layout', async () => {
    const user = userEvent.setup();
    const { client } = renderApp('three-devices');
    const tile = await screen.findByTestId('tile-d3');
    tile.focus();
    const before = client.getSnapshot().screenLayout.screens.find((s) => s.deviceId === 'd3')!;
    await user.keyboard('{ArrowDown}');
    await waitFor(() => {
      const after = client.getSnapshot().screenLayout.screens.find((s) => s.deviceId === 'd3')!;
      expect(after.y).not.toBe(before.y);
    });
  });
});

describe('Devices', () => {
  it('forgetting a computer needs confirmation', async () => {
    const user = userEvent.setup();
    const { client } = renderApp('two-devices', '/devices');
    await user.click(await screen.findByRole('button', { name: 'Forget Office PC' }));
    const alert = screen.getByRole('alertdialog', { name: 'Forget Office PC?' });
    await user.click(within(alert).getByRole('button', { name: 'Cancel' }));
    expect(client.getSnapshot().devices).toHaveLength(2);
    await user.click(screen.getByRole('button', { name: 'Forget Office PC' }));
    await user.click(screen.getByRole('button', { name: 'Forget Computer' }));
    await waitFor(() => expect(client.getSnapshot().devices).toHaveLength(1));
    expect(await screen.findByText('No other computers yet')).toBeInTheDocument();
  });

  it('explains a role conflict inline and fixes it from General', async () => {
    const user = userEvent.setup();
    const { client } = renderApp('role-conflict', '/devices');
    const err = await screen.findByTestId('link-error');
    expect(err).toHaveTextContent('Both computers are set up the same way');
    // The generic link banner stays quiet: the device explains itself.
    expect(screen.queryByTestId('banner')).not.toBeInTheDocument();
    await user.click(within(err).getByRole('button', { name: 'Change Role…' }));
    expect(window.location.hash).toBe('#/general');
    await user.click(await screen.findByRole('radio', { name: /Control this computer from another one/ }));
    await user.click(await screen.findByRole('button', { name: /Let Another Computer Control This One/ }));
    await waitFor(() =>
      expect(client.getSnapshot().devices.find((d) => d.id === 'd2')?.linkError).toBeNull(),
    );
  });

  it('switch button on a device row switches control', async () => {
    const user = userEvent.setup();
    const { client } = renderApp('two-devices', '/devices');
    await user.click(await screen.findByRole('button', { name: /^Switch$/ }));
    await waitFor(() => expect(client.getSnapshot().controlState?.activeScreen).toBe('d2'));
  });
});

describe('Incoming pairing requests', () => {
  it('are surfaced on any screen and can be accepted', async () => {
    const user = userEvent.setup();
    const { client, engine } = renderApp('two-devices', '/diagnostics');
    await screen.findByTestId('diagnostics');
    act(() => engine.simulate.incomingPairingRequest());
    const sheet = await screen.findByTestId('incoming-request');
    expect(sheet).toHaveTextContent('3f2a 91c4 8d10 6b57');
    await user.click(within(sheet).getByRole('button', { name: 'Accept' }));
    await waitFor(() => expect(client.getSnapshot().devices).toHaveLength(3));
    expect(screen.queryByTestId('incoming-request')).not.toBeInTheDocument();
  });

  it('Escape declines', async () => {
    const user = userEvent.setup();
    const { client, engine } = renderApp('two-devices');
    await screen.findByTestId('home');
    act(() => engine.simulate.incomingPairingRequest());
    await screen.findByTestId('incoming-request');
    await user.keyboard('{Escape}');
    await waitFor(() => expect(client.getSnapshot().incomingPairingRequest).toBeNull());
    expect(client.getSnapshot().devices).toHaveLength(2);
  });
});

describe('Link state banners', () => {
  it.each([
    ['reconnecting', 'Reconnecting…'],
    ['disconnected', 'Not connected'],
    ['error', 'Sharing is paused'],
    ['permission-missing', 'Flow needs permission'],
  ])('%s shows "%s"', async (scenario, title) => {
    renderApp(scenario);
    expect(await screen.findByTestId('banner')).toHaveTextContent(title);
  });

  it('connected shows no banner; Try Again recovers a disconnected link', async () => {
    const user = userEvent.setup();
    const { client } = renderApp('disconnected');
    await user.click(await screen.findByRole('button', { name: 'Try Again' }));
    await waitFor(() => expect(client.getSnapshot().linkState).toBe('connected'));
    await waitFor(() => expect(screen.queryByTestId('banner')).not.toBeInTheDocument());
  });

  it('Allow… opens OS-specific permission guidance', async () => {
    const user = userEvent.setup();
    renderApp('permission-missing');
    await user.click(await screen.findByRole('button', { name: 'Allow…' }));
    expect(await screen.findByTestId('permission-guide-macos')).toBeInTheDocument();
  });

  it('macOS lists Accessibility and Input Monitoring separately, each with its own Settings link', async () => {
    const user = userEvent.setup();
    const { client } = renderApp('permission-missing');
    const opened: string[] = [];
    const original = client.openPermissionSettings;
    client.openPermissionSettings = async (name: string) => {
      opened.push(name);
      await original(name);
    };
    await user.click(await screen.findByRole('button', { name: 'Allow…' }));
    const guide = await screen.findByTestId('permission-guide-macos');
    expect(guide).toHaveTextContent('Accessibility');
    expect(guide).toHaveTextContent('Input Monitoring');
    const buttons = within(guide).getAllByRole('button', { name: 'Open System Settings' });
    expect(buttons).toHaveLength(2);
    await user.click(buttons[1]!);
    expect(opened).toEqual(['Input Monitoring']);
  });

  it('Stop Trying cancels reconnecting for now without turning automatic reconnecting off', async () => {
    const user = userEvent.setup();
    const { client } = renderApp('reconnecting');
    await user.click(await screen.findByRole('button', { name: 'Stop Trying' }));
    await waitFor(() => expect(client.getSnapshot().linkState).toBe('disconnected'));
    expect(client.getSnapshot().settings?.autoReconnect).toBe(true);
    expect(await screen.findByRole('button', { name: 'Try Again' })).toBeInTheDocument();
  });

  it('Windows and Linux get their own guidance', async () => {
    const user = userEvent.setup();
    renderApp('permission-missing', '/home', 'linux');
    await user.click(await screen.findByRole('button', { name: 'Allow…' }));
    const guide = await screen.findByTestId('permission-guide-linux');
    expect(within(guide).getByText('sudo usermod -aG input $USER')).toBeInTheDocument();
    expect(within(guide).getAllByRole('button', { name: /^Copy command/ })).toHaveLength(2);
    expect(within(guide).getByRole('button', { name: 'Check Again' })).toBeInTheDocument();
    expect(within(guide).getByText(/Wayland/)).toHaveClass('t-footnote');
  });
});

describe('Switch HUD', () => {
  it('announces the new active screen', async () => {
    const user = userEvent.setup();
    renderApp('three-devices');
    await user.click(await screen.findByRole('button', { name: 'Switch to Office PC' }));
    expect(await screen.findByTestId('hud')).toHaveTextContent('Now controlling Office PC');
  });
});

describe('Settings', () => {
  it('Keyboard & Shortcuts: records a switch shortcut and toggles edge switching', async () => {
    const user = userEvent.setup();
    const { client } = renderApp('two-devices', '/shortcuts');
    await user.click(await screen.findByRole('button', { name: 'Switch shortcut' }));
    await user.keyboard('{Control>}{Shift>}[Space]{/Shift}{/Control}');
    await waitFor(() =>
      expect(client.getSnapshot().settings?.switchKey.keys).toEqual(['Ctrl', 'Shift', 'Space']),
    );
    await user.click(screen.getByRole('switch', { name: 'Switch at screen edges' }));
    await waitFor(() => expect(client.getSnapshot().settings?.edgeSwitching).toBe(false));
    expect(screen.getByRole('slider', { name: 'Edge switch delay' })).toBeDisabled();
  });

  it('adds a remap rule only once both keys are chosen', async () => {
    const user = userEvent.setup();
    const { client } = renderApp('two-devices', '/shortcuts');
    await user.click(await screen.findByRole('button', { name: 'Add Rule' }));
    const rules = screen.getAllByTestId('remap-rule');
    const rule = rules[rules.length - 1]!;
    await user.click(within(rule).getByRole('button', { name: 'Key to remap' }));
    await user.keyboard('[CapsLock]');
    expect(client.getSnapshot().remapRules).toHaveLength(1); // still just the seeded one
    await user.click(within(rule).getByRole('button', { name: 'Becomes' }));
    await user.keyboard('{End}');
    await waitFor(() => expect(client.getSnapshot().remapRules).toHaveLength(2));
    expect(client.getSnapshot().remapRules[1]).toEqual({ from: 'CapsLock', to: 'End', whenTargetOs: null });
  });

  it('General: appearance is applied to <html> and persisted', async () => {
    const user = userEvent.setup();
    renderApp('two-devices', '/general');
    await user.click(await screen.findByRole('radio', { name: 'Dark' }));
    expect(document.documentElement.dataset.theme).toBe('dark');
    expect(localStorage.getItem('flow.theme')).toBe('dark');
    await user.click(screen.getByRole('radio', { name: 'System' }));
    expect(document.documentElement.dataset.theme).toBeUndefined();
  });

  it('Diagnostics: shows numbers, confirms before resetting', async () => {
    const user = userEvent.setup();
    const { client } = renderApp('controlling-remote', '/diagnostics');
    expect(await screen.findByTestId('stat-Switch time (typical)')).toHaveTextContent('11 ms');
    await client.updateSettings({ debugLogging: true });
    await user.click(screen.getByRole('button', { name: 'Reset…' }));
    expect(client.getSnapshot().settings?.debugLogging).toBe(true); // not yet
    await user.click(screen.getByRole('button', { name: 'Reset Settings' }));
    await waitFor(() => expect(client.getSnapshot().settings?.debugLogging).toBe(false));
  });
});

describe('Onboarding', () => {
  it('walks welcome → role → permissions → pair → done and then shows the app', async () => {
    localStorage.clear();
    const user = userEvent.setup();
    renderApp('fresh-install', '/');
    const ob = await screen.findByTestId('onboarding');
    await user.click(screen.getByRole('button', { name: 'Get Started' }));
    expect(ob).toHaveAttribute('data-step', 'role');
    await user.click(screen.getByRole('button', { name: 'Continue' }));
    expect(ob).toHaveAttribute('data-step', 'permissions');
    expect(screen.getByRole('button', { name: 'Continue' })).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'Skip for Now' }));
    expect(ob).toHaveAttribute('data-step', 'pair');
    await user.click(await screen.findByRole('button', { name: 'Skip for Now' }));
    expect(ob).toHaveAttribute('data-step', 'done');
    expect(screen.getByLabelText('Step 5 of 5')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Start Using Flow' }));
    expect(await screen.findByTestId('home')).toBeInTheDocument();
    expect(localStorage.getItem('flow.onboarding_complete')).toBe('1');
  });
});

describe('Tray popover', () => {
  it('lists computers, marks the active one and switches on click', async () => {
    const user = userEvent.setup();
    const { client } = renderApp('three-devices', '/tray');
    await user.click(await screen.findByRole('button', { name: /Office PC/ }));
    await waitFor(() => expect(client.getSnapshot().controlState?.activeScreen).toBe('d2'));
    expect(screen.getByTestId('tray-summary')).toHaveTextContent('Controlling Office PC');
    expect(screen.getByTestId('tray-d2')).toHaveAttribute('aria-current', 'true');
    expect(screen.getByRole('button', { name: 'Return to This Computer' })).toBeInTheDocument();
  });
});

describe('friendlyError', () => {
  it('maps codes to plain language and never leaks raw messages', () => {
    expect(friendlyError(new FlowCommandError('not_controller', 'RAW'))).toMatch(/shares its keyboard/);
    expect(friendlyError(new FlowCommandError('wat', 'device not switchable: DeviceId("d2")'))).toBe(
      'Something went wrong. Please try again.',
    );
    expect(friendlyError(new Error('boom'))).toBe('Something went wrong. Please try again.');
  });
});

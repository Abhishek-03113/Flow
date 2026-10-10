import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';
import {
  Alert,
  KeyCombo,
  SegmentedControl,
  Sheet,
  ShortcutRecorder,
  Toggle,
  displayKey,
  tokenFromCode,
  labelForKeys,
} from '.';

describe('Toggle', () => {
  it('is a switch exposing its state and toggles by click and Space', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<Toggle checked={false} onChange={onChange} label="Launch at login" />);
    const sw = screen.getByRole('switch', { name: 'Launch at login' });
    expect(sw).toHaveAttribute('aria-checked', 'false');
    await user.click(sw);
    sw.focus();
    await user.keyboard(' ');
    expect(onChange).toHaveBeenCalledTimes(2);
    expect(onChange).toHaveBeenCalledWith(true);
  });
});

describe('SegmentedControl', () => {
  function Harness() {
    const [v, setV] = useState<'a' | 'b' | 'c'>('a');
    return (
      <SegmentedControl<'a' | 'b' | 'c'>
        label="Letters"
        value={v}
        onChange={setV}
        options={[
          { value: 'a', label: 'A' },
          { value: 'b', label: 'B' },
          { value: 'c', label: 'C' },
        ]}
      />
    );
  }
  it('uses radio semantics with roving tabindex and arrow keys (wrapping)', async () => {
    const user = userEvent.setup();
    render(<Harness />);
    const a = screen.getByRole('radio', { name: 'A' });
    expect(a).toHaveAttribute('tabindex', '0');
    expect(screen.getByRole('radio', { name: 'B' })).toHaveAttribute('tabindex', '-1');
    a.focus();
    await user.keyboard('{ArrowRight}');
    expect(screen.getByRole('radio', { name: 'B' })).toBeChecked();
    expect(screen.getByRole('radio', { name: 'B' })).toHaveFocus();
    await user.keyboard('{ArrowLeft}{ArrowLeft}');
    expect(screen.getByRole('radio', { name: 'C' })).toBeChecked();
  });
});

describe('KeyCap', () => {
  it('draws mac glyphs on macOS and text elsewhere, with a spoken label', () => {
    const { rerender } = render(<KeyCombo keys={['Ctrl', 'Alt', 'Shift', 'Meta', 'K']} os="macos" />);
    const combo = screen.getByRole('img');
    expect(combo).toHaveTextContent('⌃⌥⇧⌘K');
    expect(combo).toHaveAccessibleName('Control Option Shift Command K');
    rerender(<KeyCombo keys={['Ctrl', 'Meta', 'ScrollLock']} os="windows" />);
    expect(screen.getByRole('img')).toHaveTextContent('CtrlWinScroll Lock');
    expect(displayKey('Meta', 'linux').text).toBe('Super');
  });
});

describe('key tokens', () => {
  it('maps KeyboardEvent.code to canonical engine tokens', () => {
    expect(tokenFromCode('MetaLeft')).toBe('Meta');
    expect(tokenFromCode('KeyA')).toBe('A');
    expect(tokenFromCode('Digit7')).toBe('7');
    expect(tokenFromCode('F13')).toBe('F13');
    expect(tokenFromCode('ScrollLock')).toBe('ScrollLock');
    expect(tokenFromCode('Escape')).toBe('Esc');
    expect(tokenFromCode('IntlYen')).toBeNull();
    expect(labelForKeys(['Ctrl', 'Shift', 'Space'])).toBe('Ctrl + Shift + Space');
    expect(labelForKeys(['ScrollLock'])).toBe('Scroll Lock');
  });
});

describe('ShortcutRecorder', () => {
  it('records a modifier chord as canonical tokens (not glyphs)', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <ShortcutRecorder label="Switch shortcut" value={['ScrollLock']} os="macos" onChange={onChange} />,
    );
    await user.click(screen.getByRole('button', { name: 'Switch shortcut' }));
    expect(screen.getByText('Type shortcut…')).toBeInTheDocument();
    await user.keyboard('{Meta>}{Shift>}k{/Shift}{/Meta}');
    expect(onChange).toHaveBeenCalledWith({ keys: ['Shift', 'Meta', 'K'], label: 'Shift + Meta + K' });
    expect(screen.queryByText('Type shortcut…')).not.toBeInTheDocument();
  });

  it('shows held modifiers live and ignores a bare modifier', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<ShortcutRecorder label="r" value={null} os="windows" onChange={onChange} />);
    await user.click(screen.getByRole('button', { name: 'r' }));
    await user.keyboard('{Control>}');
    expect(screen.getByRole('img')).toHaveAccessibleName('Control');
    expect(onChange).not.toHaveBeenCalled();
  });

  it('accepts a bare function key (Scroll Lock / F13) and cancels on Escape', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<ShortcutRecorder label="r" value={null} os="linux" onChange={onChange} />);
    await user.click(screen.getByRole('button', { name: 'r' }));
    await user.keyboard('[ScrollLock]');
    expect(onChange).toHaveBeenLastCalledWith({ keys: ['ScrollLock'], label: 'Scroll Lock' });
    await user.click(screen.getByRole('button', { name: 'r' }));
    await user.keyboard('{Escape}');
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(screen.getByText('Not set')).toBeInTheDocument();
  });

  it('single-key mode commits the first key, modifiers included; Backspace clears when allowed', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<ShortcutRecorder label="r" mode="key" allowClear value={['A']} os="macos" onChange={onChange} />);
    await user.click(screen.getByRole('button', { name: 'r' }));
    await user.keyboard('{Meta}');
    expect(onChange).toHaveBeenLastCalledWith({ keys: ['Meta'], label: 'Meta' });
    await user.click(screen.getByRole('button', { name: 'r' }));
    await user.keyboard('{Backspace}');
    expect(onChange).toHaveBeenLastCalledWith(null);
  });
});

describe('Sheet and Alert', () => {
  it('Sheet is a labelled modal dialog; Escape closes; focus returns to the opener', async () => {
    const user = userEvent.setup();
    function Harness() {
      const [open, setOpen] = useState(false);
      return (
        <>
          <button onClick={() => setOpen(true)}>Open</button>
          <Sheet
            open={open}
            title="Add Computer"
            onClose={() => setOpen(false)}
            footer={<button>Done</button>}
          >
            <button>Inside</button>
          </Sheet>
        </>
      );
    }
    render(<Harness />);
    const opener = screen.getByRole('button', { name: 'Open' });
    await user.click(opener);
    const dialog = screen.getByRole('dialog', { name: 'Add Computer' });
    expect(dialog).toHaveAttribute('aria-modal', 'true');
    expect(screen.getByRole('button', { name: 'Inside' })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole('button', { name: 'Done' })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole('button', { name: 'Inside' })).toHaveFocus(); // wraps
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(opener).toHaveFocus();
  });

  it('Alert focuses the safe action first and exposes title and message', async () => {
    const cancel = vi.fn();
    const del = vi.fn();
    render(
      <Alert
        open
        title="Forget Office PC?"
        message="You can pair it again later."
        onClose={cancel}
        actions={[
          { label: 'Forget', variant: 'destructive-prominent', onClick: del },
          { label: 'Cancel', onClick: cancel, autoFocus: true },
        ]}
      />,
    );
    const alert = screen.getByRole('alertdialog', { name: 'Forget Office PC?' });
    expect(alert).toHaveAccessibleDescription('You can pair it again later.');
    expect(screen.getByRole('button', { name: 'Cancel' })).toHaveFocus();
    await userEvent.setup().keyboard('{Escape}');
    expect(cancel).toHaveBeenCalled();
    expect(del).not.toHaveBeenCalled();
  });
});

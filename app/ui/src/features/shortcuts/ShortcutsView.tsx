import { FolderOpen, Plus, Trash2 } from 'lucide-react';
import { useEffect, useState } from 'react';
import type { HostOs, RemapRule } from '../../client/types';
import { friendlyError } from '../../copy';
import {
  Button,
  Row,
  Section,
  Select,
  ShortcutRecorder,
  Slider,
  spokenKeys,
  Toggle,
  Tooltip,
  useToast,
} from '../../design';
import { useClient, useFlow } from '../../state/FlowProvider';
import { localOs, myRole } from '../../state/selectors';
import { useAction } from '../../state/useAction';

const PRESETS = [
  { value: 'ScrollLock', label: 'Scroll Lock', keys: ['ScrollLock'] },
  { value: 'Pause', label: 'Pause', keys: ['Pause'] },
  { value: 'F13', label: 'F13', keys: ['F13'] },
  { value: 'Ctrl+Shift+Space', label: 'Control Shift Space', keys: ['Ctrl', 'Shift', 'Space'] },
];

type Draft = { from: string; to: string; whenTargetOs: HostOs | null };

function RuleEditor({
  rule,
  os,
  onChange,
  onDelete,
}: {
  rule: Draft;
  os: HostOs;
  onChange(r: Draft): void;
  onDelete(): void;
}) {
  const targetOs = rule.whenTargetOs ?? os;
  return (
    <div className="fl-row" data-testid="remap-rule" style={{ flexWrap: 'wrap', alignItems: 'flex-end' }}>
      <div className="fl-field">
        <span className="fl-field__label">{rule.from ? `Key: ${spokenKeys([rule.from], os)}` : 'Key'}</span>
        <ShortcutRecorder
          mode="key"
          label="Key to remap"
          value={rule.from ? [rule.from] : null}
          emptyText="Choose key"
          os={os}
          onChange={(b) => b && onChange({ ...rule, from: b.keys[0]! })}
        />
      </div>
      <span aria-hidden className="t-secondary" style={{ paddingBottom: 6 }}>
        →
      </span>
      <div className="fl-field">
        <span className="fl-field__label">
          {rule.to ? `Becomes: ${spokenKeys([rule.to], targetOs)}` : 'Becomes'}
        </span>
        <ShortcutRecorder
          mode="key"
          label="Becomes"
          value={rule.to ? [rule.to] : null}
          emptyText="Choose key"
          os={targetOs}
          onChange={(b) => b && onChange({ ...rule, to: b.keys[0]! })}
        />
      </div>
      <span style={{ flex: 1 }} />
      <div className="fl-field">
        <span className="fl-field__label">When controlling</span>
        <Select<'any' | HostOs>
          label="When controlling"
          value={rule.whenTargetOs ?? 'any'}
          onChange={(v) => onChange({ ...rule, whenTargetOs: v === 'any' ? null : v })}
          options={[
            { value: 'any', label: 'Any computer' },
            { value: 'macos', label: 'macOS' },
            { value: 'windows', label: 'Windows' },
            { value: 'linux', label: 'Linux' },
          ]}
        />
      </div>
      <Tooltip label="Remove rule">
        <Button
          size="small"
          aria-label="Remove rule"
          icon={<Trash2 size={14} strokeWidth={1.6} />}
          onClick={onDelete}
        />
      </Tooltip>
    </div>
  );
}

export function ShortcutsView() {
  const s = useFlow();
  const client = useClient();
  const toast = useToast();
  const { run } = useAction();
  const os = localOs(s);
  const settings = s.settings;
  const [rules, setRules] = useState<Draft[]>(s.remapRules);
  useEffect(
    () => setRules((cur) => [...s.remapRules, ...cur.filter((r) => !r.from || !r.to)]),
    [s.remapRules],
  );

  if (!settings) return null;
  const preset =
    PRESETS.find((p) => p.keys.join('+') === settings.switchKey.keys.join('+'))?.value ?? 'custom';

  const saveRules = async (next: Draft[]) => {
    setRules(next);
    const complete = next.filter((r) => r.from && r.to) as RemapRule[];
    if (complete.length !== next.length) return; // unfinished draft stays local
    try {
      await client.setRemapRules(complete);
    } catch (e) {
      toast.show(friendlyError(e), 'error');
    }
  };

  const delayText = settings.edgeSwitchDelayMs === 0 ? 'Instant' : `${settings.edgeSwitchDelayMs} ms`;

  return (
    <div data-testid="shortcuts">
      {myRole(s) === 'controlled' && (
        <p className="t-callout t-secondary" style={{ margin: '0 var(--s-4) var(--s-4)' }}>
          This computer is controlled by another one, so these settings take effect on the computer that
          shares its keyboard and mouse.
        </p>
      )}
      <Section
        title="Switch computers"
        footer="Your shortcut always works, even while another computer is in use. Click it, then press the keys you want."
      >
        <Row
          title="Switch shortcut"
          subtitle="Moves to the next computer."
          trailing={
            <ShortcutRecorder
              label="Switch shortcut"
              value={settings.switchKey.keys}
              os={os}
              onChange={(b) => b && void run(() => client.setSwitchKey(b), 'Shortcut updated')}
            />
          }
        />
        <Row
          title="Preset"
          trailing={
            <Select<string>
              label="Shortcut preset"
              value={preset}
              options={[
                ...(preset === 'custom' ? [{ value: 'custom', label: 'Custom' }] : []),
                ...PRESETS.map(({ value, label }) => ({ value, label })),
              ]}
              onChange={(v) => {
                const p = PRESETS.find((x) => x.value === v);
                if (p)
                  void run(() => client.setSwitchKey({ label: p.label, keys: p.keys }), 'Shortcut updated');
              }}
            />
          }
        />
        <Row
          title="Return home shortcut"
          subtitle="Optional. Jump straight back to this computer."
          trailing={
            <>
              <ShortcutRecorder
                label="Return home shortcut"
                value={settings.returnHomeKey?.keys ?? null}
                os={os}
                allowClear
                emptyText="Not set"
                onChange={(b) => void run(() => client.updateSettings({ returnHomeKey: b }))}
              />
            </>
          }
        />
      </Section>

      <Section
        title="Screen edges"
        footer="Move the pointer past the edge of a screen to continue on the computer next to it. Arrange screens on the Home page."
      >
        <Row
          title="Switch at screen edges"
          trailing={
            <Toggle
              label="Switch at screen edges"
              checked={settings.edgeSwitching}
              onChange={(v) => void run(() => client.updateSettings({ edgeSwitching: v }))}
            />
          }
        />
        <Row
          title="Delay"
          subtitle="Wait before crossing, to avoid switching by accident."
          trailing={
            <>
              <Slider
                label="Edge switch delay"
                min={0}
                max={1000}
                step={50}
                value={settings.edgeSwitchDelayMs}
                valueText={delayText}
                disabled={!settings.edgeSwitching}
                onChange={(v) => void run(() => client.updateSettings({ edgeSwitchDelayMs: v }))}
              />
              <span className="t-callout" style={{ minWidth: 52, textAlign: 'right' }} aria-hidden>
                {delayText}
              </span>
            </>
          }
        />
      </Section>

      <Section
        title="Remap keys"
        footer="Rules apply to keys sent to other computers. A common one: turn ⌘ into Ctrl when controlling a Windows computer."
      >
        {rules.length === 0 && <Row title={<span className="t-secondary">No rules yet</span>} />}
        {rules.map((r, i) => (
          <li key={i} className="fl-row-wrap">
            <RuleEditor
              rule={r}
              os={os}
              onChange={(n) => void saveRules(rules.map((x, j) => (j === i ? n : x)))}
              onDelete={() => void saveRules(rules.filter((_, j) => j !== i))}
            />
          </li>
        ))}
        <Row
          title={
            <Button
              variant="plain"
              icon={<Plus size={14} strokeWidth={1.8} />}
              onClick={() => setRules([...rules, { from: '', to: '', whenTargetOs: null }])}
            >
              Add Rule
            </Button>
          }
        />
      </Section>

      <Section title="Advanced">
        <Row
          icon={<FolderOpen size={16} strokeWidth={1.6} />}
          title="Keymap folder"
          subtitle={<span className="selectable">{settings.keymapOverridesPath || 'Not available yet'}</span>}
          trailing={
            <Button size="small" onClick={() => void run(() => client.revealConfigDir())}>
              Open Keymap Folder
            </Button>
          }
        />
      </Section>
    </div>
  );
}

import {
  Activity,
  AlertTriangle,
  Keyboard,
  Laptop,
  Monitor,
  Plus,
  Settings,
  Trash2,
  Wifi,
} from "lucide-react";
import { useState } from "react";
import {
  Alert,
  Badge,
  CodeBlock,
  Banner,
  Button,
  EmptyState,
  Hud,
  KeyCombo,
  Row,
  SegmentedControl,
  Section,
  Select,
  ShortcutRecorder,
  Sidebar,
  SidebarItem,
  Slider,
  Spinner,
  StatusDot,
  Toggle,
  Tooltip,
  useTheme,
  useToast,
  type ThemeChoice,
} from "../src";

const ICON = { size: 16, strokeWidth: 1.6 } as const;

function Card({
  title,
  children,
  wide,
}: {
  title: string;
  children: React.ReactNode;
  wide?: boolean;
}) {
  return (
    <section
      style={{
        breakInside: "avoid",
        marginBottom: "var(--s-5)",
        gridColumn: wide ? "span 2" : undefined,
      }}
    >
      <h2 className="fl-section__title">{title}</h2>
      <div
        style={{
          background: "var(--bg-secondary)",
          borderRadius: "var(--r-card)",
          padding: "var(--s-4)",
          boxShadow: "var(--shadow-1)",
          display: "flex",
          flexDirection: "column",
          gap: "var(--s-3)",
        }}
      >
        {children}
      </div>
    </section>
  );
}

const line: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "var(--s-3)",
  flexWrap: "wrap",
};

/** Living reference for every component at `#/design`. */
export function DesignGallery() {
  const { theme, setTheme } = useTheme();
  const toast = useToast();
  const [on, setOn] = useState(true);
  const [seg, setSeg] = useState<ThemeChoice>("system");
  const [sel, setSel] = useState("any");
  const [slide, setSlide] = useState(120);
  const [alert, setAlert] = useState(false);
  const [keys, setKeys] = useState<string[] | null>(["Ctrl", "Shift", "Space"]);

  return (
    <div
      style={{ height: "100%", overflow: "auto", padding: "var(--s-6)" }}
      data-testid="gallery"
    >
      <div
        style={{
          ...line,
          justifyContent: "space-between",
          marginBottom: "var(--s-5)",
        }}
      >
        <h1 className="t-large-title">Design system</h1>
        <SegmentedControl<ThemeChoice>
          label="Theme"
          value={theme}
          onChange={setTheme}
          options={[
            { value: "system", label: "System" },
            { value: "light", label: "Light" },
            { value: "dark", label: "Dark" },
          ]}
        />
      </div>
      <div style={{ columnCount: 3, columnGap: "var(--s-5)" }}>
        <Card title="Type scale">
          <p className="t-large-title">Large Title 26</p>
          <p className="t-title">Title 22</p>
          <p className="t-title3">Title 3 · 15 semibold</p>
          <p className="t-headline">Headline 13 semibold</p>
          <p className="t-body">Body 13 regular</p>
          <p className="t-callout t-secondary">Callout 12 secondary</p>
          <p className="t-footnote t-secondary">Footnote 11 secondary</p>
        </Card>
        <Card title="Buttons">
          <div style={line}>
            <Button>Bordered</Button>
            <Button variant="prominent">Prominent</Button>
            <Button variant="plain">Plain</Button>
          </div>
          <div style={line}>
            <Button variant="destructive-prominent">Delete</Button>
            <Button tone="destructive">Forget…</Button>
            <Button icon={<Plus {...ICON} />}>Add</Button>
            <Button variant="prominent" loading>
              Working
            </Button>
          </div>
          <div style={line}>
            <Tooltip label="Forget this computer">
              <Button
                size="small"
                aria-label="Forget"
                icon={<Trash2 size={14} strokeWidth={1.6} />}
              />
            </Tooltip>
            <Button disabled>Disabled</Button>
            <Button size="small">Small</Button>
          </div>
        </Card>
        <Card title="Controls">
          <div style={line}>
            <Toggle checked={on} onChange={setOn} label="Example switch" />
            <Toggle
              checked={false}
              onChange={() => undefined}
              label="Off switch"
            />
            <Toggle
              checked
              disabled
              onChange={() => undefined}
              label="Disabled switch"
            />
          </div>
          <SegmentedControl<ThemeChoice>
            label="Example"
            value={seg}
            onChange={setSeg}
            options={[
              { value: "system", label: "System" },
              { value: "light", label: "Light" },
              { value: "dark", label: "Dark" },
            ]}
          />
          <div style={line}>
            <Select
              label="Target"
              value={sel}
              onChange={setSel}
              options={[
                { value: "any", label: "Any computer" },
                { value: "macos", label: "macOS" },
                { value: "windows", label: "Windows" },
              ]}
            />
            <Slider
              label="Delay"
              min={0}
              max={1000}
              step={50}
              value={slide}
              onChange={setSlide}
            />
            <span className="t-callout t-secondary">{slide} ms</span>
          </div>
        </Card>
        <Card title="Key caps">
          <div style={line}>
            <KeyCombo keys={["Meta", "Shift", "K"]} os="macos" />
            <KeyCombo keys={["Meta", "Shift", "K"]} os="windows" />
            <KeyCombo keys={["Ctrl", "Alt", "ScrollLock"]} os="linux" />
          </div>
          <KeyCombo keys={["ScrollLock"]} os="macos" large />
          <ShortcutRecorder
            label="Shortcut"
            value={keys}
            os="macos"
            onChange={(b) => setKeys(b?.keys ?? null)}
            allowClear
          />
        </Card>
        <Card title="Status">
          <div style={line}>
            <StatusDot tone="green" /> <span>Connected</span>
            <StatusDot tone="orange" pulse /> <span>Reconnecting</span>
            <StatusDot tone="red" /> <span>Error</span>
          </div>
          <div style={line}>
            <Badge>Controlled</Badge>
            <Badge tone="accent">Active</Badge>
            <Badge tone="green">Connected</Badge>
            <Badge tone="orange">Needs attention</Badge>
            <Badge tone="red">Offline</Badge>
          </div>
          <CodeBlock>sudo usermod -aG input $USER</CodeBlock>
          <div style={line}>
            <Spinner label="Loading" />
            <Spinner size={24} />
          </div>
        </Card>
        <Card title="Sidebar">
          <Sidebar>
            <SidebarItem
              icon={<Laptop {...ICON} />}
              label="Devices"
              active
              onClick={() => undefined}
            />
            <SidebarItem
              icon={<Keyboard {...ICON} />}
              label="Keyboard & Shortcuts"
              onClick={() => undefined}
            />
            <SidebarItem
              icon={<Activity {...ICON} />}
              label="Diagnostics"
              onClick={() => undefined}
            />
          </Sidebar>
        </Card>
        <Card title="Grouped list" wide>
          <Section
            title="Computers"
            footer="Rows follow macOS System Settings."
          >
            <Row
              icon={<Laptop {...ICON} />}
              iconTone="accent"
              title="MacBook Pro"
              subtitle="macOS · This computer"
              trailing={<Badge tone="accent">Active</Badge>}
            />
            <Row
              icon={<Monitor {...ICON} />}
              iconTone="gray"
              title="Office PC"
              subtitle="Windows · Connected"
              trailing={<Button size="small">Switch</Button>}
            />
            <Row
              icon={<Settings {...ICON} />}
              title="Clickable row"
              onClick={() => toast.show("Row clicked")}
              chevron
            />
          </Section>
        </Card>
        <Card title="Banners">
          <Banner
            tone="orange"
            icon={<Wifi {...ICON} />}
            title="Reconnecting…"
            actions={<Button size="small">Cancel</Button>}
          >
            Lost the connection to Office PC.
          </Banner>
          <Banner
            tone="red"
            icon={<AlertTriangle {...ICON} />}
            title="Something went wrong"
            actions={<Button size="small">Retry</Button>}
          >
            Sharing is paused.
          </Banner>
        </Card>
        <Card title="Empty state">
          <EmptyState
            art={<Laptop size={40} strokeWidth={1.2} />}
            title="No other computers yet"
            actions={<Button variant="prominent">Add Computer…</Button>}
          >
            Pair another computer to share this keyboard and mouse.
          </EmptyState>
        </Card>
        <Card title="Overlays">
          <div style={line}>
            <Button onClick={() => toast.show("Settings saved")}>
              Show toast
            </Button>
            <Button onClick={() => setAlert(true)}>Show alert</Button>
          </div>
          <Hud
            inline
            icon={<Monitor size={30} strokeWidth={1.4} />}
            title="Now controlling Office PC"
          />
        </Card>
      </div>
      <Alert
        open={alert}
        title="Forget Office PC?"
        message="Flow will stop sharing with this computer. You can pair it again later."
        onClose={() => setAlert(false)}
        actions={[
          {
            label: "Forget Computer",
            variant: "destructive-prominent",
            onClick: () => setAlert(false),
          },
          { label: "Cancel", onClick: () => setAlert(false), autoFocus: true },
        ]}
      />
    </div>
  );
}

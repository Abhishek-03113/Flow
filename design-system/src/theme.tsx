import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

export type ThemeChoice = "system" | "light" | "dark";
const KEY = "flow.theme";

function read(): ThemeChoice {
  try {
    const v = localStorage.getItem(KEY);
    return v === "light" || v === "dark" ? v : "system";
  } catch {
    return "system";
  }
}

const Ctx = createContext<{
  theme: ThemeChoice;
  setTheme(t: ThemeChoice): void;
}>({
  theme: "system",
  setTheme: () => undefined,
});
export const useTheme = () => useContext(Ctx);

/** System by default; "light"/"dark" force `<html data-theme>` (tokens.css handles both). */
export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<ThemeChoice>(read);
  useEffect(() => {
    const root = document.documentElement;
    if (theme === "system") root.removeAttribute("data-theme");
    else root.setAttribute("data-theme", theme);
  }, [theme]);
  const value = useMemo(
    () => ({
      theme,
      setTheme: (t: ThemeChoice) => {
        setThemeState(t);
        try {
          if (t === "system") localStorage.removeItem(KEY);
          else localStorage.setItem(KEY, t);
        } catch {
          /* private mode: keep the in-memory choice */
        }
      },
    }),
    [theme],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

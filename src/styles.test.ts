import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const styles = readFileSync(`${process.cwd()}/src/styles.css`, "utf8");

describe("UI typography", () => {
  it("uses the Windows VS Code-style UI font stack", () => {
    expect(styles).toContain(
      '--font-ui: "Segoe WPC", "Segoe UI", "Yu Gothic UI", "Meiryo UI", Meiryo, sans-serif;',
    );
    expect(styles).not.toContain("Yu Mincho");
    expect(styles).not.toContain('font-feature-settings: "palt"');
    expect(styles).toContain("letter-spacing: normal;");
  });

  it("keeps secondary text high-contrast in both themes", () => {
    expect(styles).toContain("--text: #ffffff;");
    expect(styles).toContain("--text-soft: #d0d0d0;");
    expect(styles).toContain("--text-faint: #9a9a9a;");
    expect(styles).toMatch(
      /:root\[data-theme="light"\][\s\S]*--text: #000000;[\s\S]*--text-soft: #242424;[\s\S]*--text-faint: #565656;/,
    );
  });
});

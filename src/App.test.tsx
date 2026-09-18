import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import App from "./App";

describe("browser-only safety boundary", () => {
  beforeEach(() => {
    localStorage.clear();
    delete window.__TAURI_INTERNALS__;
  });

  it("shows the icon-free project home and never implies durable storage in a browser", () => {
    const { container } = render(<App />);
    expect(screen.getByRole("heading", { name: "SANCTUM" })).toBeInTheDocument();
    expect(screen.getByText("保存機能はデスクトップ版でのみ利用できる")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "新規作成" })).toBeDisabled();
    expect(container.querySelector("svg")).not.toBeInTheDocument();
  });
});

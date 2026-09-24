import { save } from "@tauri-apps/plugin-dialog";
import { readFile, writeFile } from "@tauri-apps/plugin-fs";

// Android's document picker returns a content URI. The fs plugin understands
// that URI; plain Rust std::fs and path-based browser downloads do not.
export async function saveMobileFile(path: string, fileName: string, expectedSha256?: string): Promise<boolean> {
  const destination = await save({ title: "保存先を選択", defaultPath: fileName });
  if (!destination) return false;
  const bytes = await readFile(path);
  if (bytes.byteLength > 64 * 1024 * 1024) throw new Error("64 MBを超えるファイルはこの版では書き出せない");
  await writeFile(destination, bytes);
  const written = await readFile(destination);
  if (written.byteLength !== bytes.byteLength) throw new Error("書き出したファイルを確認できなかった");
  const digest = await crypto.subtle.digest("SHA-256", written.slice().buffer);
  const actual = Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
  const sourceDigest = expectedSha256?.toLowerCase() ?? Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes.slice().buffer)), (byte) => byte.toString(16).padStart(2, "0")).join("");
  if (actual !== sourceDigest) throw new Error("書き出したファイルの検証に失敗した");
  return true;
}

import { save } from "@tauri-apps/plugin-dialog";
import { readFile, writeFile } from "@tauri-apps/plugin-fs";

// Android's document picker returns a content URI. The fs plugin understands
// that URI; plain Rust std::fs and path-based browser downloads do not.
export async function saveMobileFile(path: string, fileName: string): Promise<boolean> {
  const destination = await save({ title: "保存先を選択", defaultPath: fileName });
  if (!destination) return false;
  const bytes = await readFile(path);
  if (bytes.byteLength > 64 * 1024 * 1024) throw new Error("64 MBを超えるファイルはこの版では書き出せない");
  await writeFile(destination, bytes);
  // Some Android WebView origins do not expose crypto.subtle. Comparing the
  // complete read-back bytes also detects a same-size damaged export.
  const written = await readFile(destination);
  if (written.byteLength !== bytes.byteLength || written.some((byte, index) => byte !== bytes[index])) {
    throw new Error("書き出したファイルの検証に失敗した");
  }
  return true;
}

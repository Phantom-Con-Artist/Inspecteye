// Copies static/ (HTML, CSS) next to the compiled scripts in dist/.
import { cpSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
mkdirSync(join(root, "dist"), { recursive: true });
cpSync(join(root, "static"), join(root, "dist"), { recursive: true });

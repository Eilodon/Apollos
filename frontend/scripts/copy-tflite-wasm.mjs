import { cp, mkdir, readdir, stat } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const tfliteWasmDir = path.join(root, 'node_modules', '@tensorflow', 'tfjs-tflite', 'wasm');
const tfCoreMinJs = path.join(root, 'node_modules', '@tensorflow', 'tfjs-core', 'dist', 'tf-core.min.js');
const tfBackendCpuMinJs = path.join(root, 'node_modules', '@tensorflow', 'tfjs-backend-cpu', 'dist', 'tf-backend-cpu.min.js');
const tfTfliteMinJs = path.join(root, 'node_modules', '@tensorflow', 'tfjs-tflite', 'dist', 'tf-tflite.min.js');
const targetDir = path.join(root, 'public', 'tflite-wasm');

async function exists(dir) {
  try {
    await stat(dir);
    return true;
  } catch {
    return false;
  }
}

async function main() {
  if (!(await exists(tfliteWasmDir))) {
    console.warn('skip copy-tflite-wasm: source dir not found:', tfliteWasmDir);
    return;
  }

  await mkdir(targetDir, { recursive: true });
  await cp(tfliteWasmDir, targetDir, { recursive: true, force: true });
  await cp(tfCoreMinJs, path.join(targetDir, 'tf-core.min.js'), { force: true });
  await cp(tfBackendCpuMinJs, path.join(targetDir, 'tf-backend-cpu.min.js'), { force: true });
  await cp(tfTfliteMinJs, path.join(targetDir, 'tf-tflite.min.js'), { force: true });
  const files = await readdir(targetDir);
  console.log(`copied ${files.length} wasm runtime files -> ${targetDir}`);
}

await main();

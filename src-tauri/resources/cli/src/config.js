import { readFileSync, writeFileSync, mkdirSync, existsSync, renameSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { homedir } from 'node:os';

const CONFIG_DIR = process.env.VIBE_USAGE_CONFIG_DIR?.trim() || join(homedir(), '.vibe-usage');
const isDev = process.env.VIBE_USAGE_DEV === '1';
const CONFIG_FILE = join(CONFIG_DIR, isDev ? 'config.dev.json' : 'config.json');

function backupPath(path) {
  return `${path}.directory-backup-${Date.now()}`;
}

function moveDirectoryOutOfFilePath(path) {
  try {
    if (statSync(path).isDirectory()) {
      renameSync(path, backupPath(path));
    }
  } catch (err) {
    if (err?.code !== 'ENOENT') throw err;
  }
}

export function getConfigPath() {
  return CONFIG_FILE;
}

export function loadConfig() {
  if (!existsSync(CONFIG_FILE)) return null;
  try {
    if (!statSync(CONFIG_FILE).isFile()) return null;
    return JSON.parse(readFileSync(CONFIG_FILE, 'utf-8'));
  } catch {
    return null;
  }
}

export function saveConfig(config) {
  mkdirSync(CONFIG_DIR, { recursive: true });
  moveDirectoryOutOfFilePath(CONFIG_FILE);
  writeFileSync(CONFIG_FILE, JSON.stringify(config, null, 2) + '\n', 'utf-8');
}

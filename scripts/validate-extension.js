import { readFile, access } from 'node:fs/promises';
import { constants } from 'node:fs';
import { resolve } from 'node:path';

const REQUIRED_PERMISSIONS = new Set(["storage", "clipboardWrite", "contextMenus"]);
const REQUIRED_CONTEXT_MENU_TITLE = "Append selection to clipboard stack";

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

async function ensureFileExists(path) {
  await access(path, constants.F_OK);
}

async function validateManifest(manifestPath) {
  const manifestRaw = await readFile(manifestPath, "utf8");
  const manifest = JSON.parse(manifestRaw);

  assert(manifest.manifest_version === 3, "Manifest must be version 3");

  const { permissions = [] } = manifest;
  for (const permission of REQUIRED_PERMISSIONS) {
    assert(
      permissions.includes(permission),
      `Manifest missing required permission: ${permission}`
    );
  }

  const background = manifest.background || {};
  assert(background.service_worker === "background.js", "Service worker path mismatch");
  assert(background.type === "module", "Service worker must be declared as an ES module");

  const contentScripts = Array.isArray(manifest.content_scripts) ? manifest.content_scripts : [];
  assert(contentScripts.length > 0, "Content script configuration missing");
  const [firstContentScript] = contentScripts;
  assert(
    Array.isArray(firstContentScript.matches) && firstContentScript.matches.includes("<all_urls>"),
    "Content script must match <all_urls>"
  );
  assert(
    Array.isArray(firstContentScript.js) && firstContentScript.js.includes("content.js"),
    "Content script must include content.js"
  );

  return manifest;
}

async function validateBackground(backgroundPath) {
  const backgroundSource = await readFile(backgroundPath, "utf8");
  assert(
    backgroundSource.includes("chrome.contextMenus.create"),
    "Background script must create a context menu"
  );
  assert(
    backgroundSource.includes(REQUIRED_CONTEXT_MENU_TITLE),
    "Background script must register the expected context menu title"
  );
  assert(
    backgroundSource.includes("appendSelection"),
    "Background script must append selections to stored clipboard data"
  );
}

async function validateReadme(readmePath) {
  const readmeContents = await readFile(readmePath, "utf8");
  assert(
    readmeContents.includes("Append selection to clipboard stack"),
    "README should mention the context menu entry so users know how to trigger the extension"
  );
}

async function main() {
  const manifestPath = resolve("extension", "manifest.json");
  const backgroundPath = resolve("extension", "background.js");
  const contentPath = resolve("extension", "content.js");
  const readmePath = resolve("README.md");

  await ensureFileExists(manifestPath);
  await ensureFileExists(backgroundPath);
  await ensureFileExists(contentPath);

  await validateManifest(manifestPath);
  await validateBackground(backgroundPath);
  await validateReadme(readmePath);
}

main().catch((error) => {
  console.error("Extension validation failed:", error.message);
  process.exitCode = 1;
});

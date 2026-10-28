// Writes the release version from RELEASE_VERSION into the three files
// that must stay in sync: the Cargo workspace, tauri.conf.json, and the
// desktop package.json. Cargo.lock entries for the workspace crates are
// updated to the same version so a later --locked build still resolves.
import { readFileSync, writeFileSync } from "node:fs";

const version = process.env.RELEASE_VERSION ?? "";
if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.]+)?$/.test(version)) {
  console.error("RELEASE_VERSION must look like X.Y.Z");
  process.exit(1);
}

const cargoPath = "Cargo.toml";
const cargo = readFileSync(cargoPath, "utf8");
let cargoMatches = 0;
const cargoNext = cargo.replace(
  /(\[workspace\.package\][^\[]*?^version = ")[^"]+(")/m,
  (_match, prefix, suffix) => {
    cargoMatches += 1;
    return `${prefix}${version}${suffix}`;
  },
);
if (cargoMatches !== 1) {
  console.error("Could not update [workspace.package] version in Cargo.toml");
  process.exit(1);
}
writeFileSync(cargoPath, cargoNext);

const lockPath = "Cargo.lock";
const lock = readFileSync(lockPath, "utf8");
let lockMatches = 0;
const lockNext = lock.replace(
  /(name = "quota-(?:cli|core|desktop)"\r?\nversion = ")[^"]+(")/g,
  (_match, prefix, suffix) => {
    lockMatches += 1;
    return `${prefix}${version}${suffix}`;
  },
);
if (lockMatches !== 3) {
  console.error("Could not update workspace crate versions in Cargo.lock");
  process.exit(1);
}
writeFileSync(lockPath, lockNext);

for (const path of [
  "apps/desktop/src-tauri/tauri.conf.json",
  "apps/desktop/package.json",
]) {
  const text = readFileSync(path, "utf8");
  let matches = 0;
  const next = text.replace(/("version"\s*:\s*")[^"]+(")/, (_match, prefix, suffix) => {
    matches += 1;
    return `${prefix}${version}${suffix}`;
  });
  if (matches !== 1) {
    console.error(`Could not update version in ${path}`);
    process.exit(1);
  }
  writeFileSync(path, next);
}

console.log(`Set app version to ${version}`);

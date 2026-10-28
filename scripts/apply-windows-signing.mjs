// Optional Windows Authenticode setup for the release workflow.
// All three environment variables must be set, or none of them.
// This script only writes the thumbprint into tauri.conf.json. The
// workflow imports the certificate into the runner's certificate store.
import { readFileSync, writeFileSync } from "node:fs";

const certificate = process.env.WINDOWS_CERTIFICATE ?? "";
const password = process.env.WINDOWS_CERTIFICATE_PASSWORD ?? "";
const thumbprint = (process.env.WINDOWS_CERTIFICATE_THUMBPRINT ?? "").trim();
const present = [certificate, password, thumbprint].filter((value) => value.trim() !== "");

if (present.length === 0) {
  console.log("Windows code-signing secrets are unset. Installers will be unsigned.");
  process.exit(0);
}
if (present.length !== 3) {
  console.error(
    "Set WINDOWS_CERTIFICATE, WINDOWS_CERTIFICATE_PASSWORD, and WINDOWS_CERTIFICATE_THUMBPRINT together, or leave all of them unset.",
  );
  process.exit(1);
}
if (!/^[0-9a-fA-F]+$/.test(thumbprint)) {
  console.error("WINDOWS_CERTIFICATE_THUMBPRINT is not a hex thumbprint.");
  process.exit(1);
}

const path = "apps/desktop/src-tauri/tauri.conf.json";
const config = JSON.parse(readFileSync(path, "utf8"));
config.bundle ??= {};
config.bundle.windows = {
  ...(config.bundle.windows ?? {}),
  certificateThumbprint: thumbprint,
  digestAlgorithm: "sha256",
  timestampUrl: "http://timestamp.digicert.com",
};
writeFileSync(path, `${JSON.stringify(config, null, 2)}\n`);
console.log("Configured tauri.conf.json for Windows code signing.");

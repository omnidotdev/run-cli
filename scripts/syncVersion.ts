/**
 * @file Sync version from `package.json` to all `Cargo.toml` files in workspace.
 * Run with `bun scripts/syncVersion.ts`
 */

import { Glob } from "bun";

const pkg = await Bun.file("package.json").json();
const version = pkg.version;

const cargoFiles = [
  "Cargo.toml",
  ...new Glob("crates/*/Cargo.toml").scanSync("."),
];

for (const path of cargoFiles) {
  const cargo = await Bun.file(path).text();

  const updatedCargo = cargo.replace(
    /^version\s*=\s*"[^"]*"/m,
    `version = "${version}"`,
  );

  await Bun.write(path, updatedCargo);
}

console.warn(
  `Synced version ${version} to ${cargoFiles.length} Cargo.toml files`,
);

#!/usr/bin/env -S deno run -A
// Copyright 2018-2024 the Deno authors. All rights reserved. MIT license.

import { $, Crate, Repo } from "@deno/rust-automation";

const repo = await Repo.load({
  name: "deno_ast",
  path: $.path(import.meta.url)
    .parentOrThrow()
    .parentOrThrow()
    .resolve(),
});

const crate = repo.getCrate("deno_ast");
const oxcDeps = crate.dependencies.filter(
  (dep) => dep.name === "oxc" || dep.name.startsWith("oxc_"),
);
for (const dep of oxcDeps) {
  const version = await Crate.getLatestVersion(dep.name);
  if (version == null) {
    throw new Error(`Could not find latest version for ${dep.name}`);
  }

  if (version !== dep.req) {
    $.logStep("Updating", dep.name, "from", dep.req, "to", version);
    crate.setDependencyVersion(dep.name, version);
  }
}

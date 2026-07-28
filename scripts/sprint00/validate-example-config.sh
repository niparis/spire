#!/usr/bin/env bash
set -euo pipefail

ruby -ryaml -e '
  config = YAML.safe_load(File.read(ARGV.fetch(0)), aliases: false)
  required = %w[schema_version linear github cloudflare harnesses dispatch concurrency security]
  missing = required - config.keys
  abort("missing root keys: #{missing.join(", ")}") unless missing.empty?
  abort("schema_version must be 1") unless config["schema_version"] == 1
  abort("dispatch policy_version must be 1") unless config.dig("dispatch", "policy_version") == 1
  abort("example must not contain deployable rules") unless config.dig("dispatch", "rules") == []
  abort("example must not contain deployable complexity mapping") unless config.dig("linear", "complexity_mapping") == {}
  abort("reviewer_can_push must be false") unless config.dig("security", "reviewer_can_push") == false
  abort("credential_can_merge must be false") unless config.dig("security", "credential_can_merge") == false
  puts "example config is valid and intentionally non-deployable"
' config/spire.example.yaml

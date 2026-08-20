# Eclipse development process ownership

- Run development and validation processes in the foreground whenever possible.
- Codex must set `ECLIPSE_PROCESS_OWNER=codex` when it runs `pnpm dev`. This lets the dev launcher remove only stale processes from an earlier Codex-owned run and prevents duplicate backend/Vite instances.
- For a temporary standalone command, use `node scripts/managed-process.mjs run --name <unique-name> --owner codex --log logs/codex-validation/<name>.log -- <command> <args...>`. Keep the wrapper in the foreground and stop it before completing the task.
- Before handing control back, run `pnpm dev:cleanup:codex` and verify that project ports and project-owned Eclipse, Vite, FFmpeg, and watcher processes are gone.
- Never daemonize a dev process silently. If manual acceptance genuinely requires a persistent instance, state that explicitly and provide its visible terminal or exact log path.
- Never kill processes merely because they are Rust, Node, FFmpeg, browser processes, or use a familiar port. Cleanup is limited to a matching process lease whose owner, executable identity, and creation time verify that this repository launched it.
- Processes started by the user use the default `interactive` owner. Leave them alone.

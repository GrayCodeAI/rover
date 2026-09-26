# Pinned upstream source and license audit

Audit snapshot date: 2026-09-25. These are exact `origin/HEAD` commits fetched
from the public repositories. The working snapshots are kept outside Rover at
`/private/tmp/rover-upstream-audit-{herdr,luvus,orca}` with `--no-checkout`;
no upstream repository was modified. No Herdr, Luvus, or Orca application code
has been copied. The 22 Herdr TOML detection manifests listed below are copied
unchanged as data, with the exact root license and file-level hashes retained.

## Immutable source revisions

| Project | Repository | Commit | Snapshot root license | Same-commit README statement |
|---|---|---|---|---|
| Herdr | [herdrdev/herdr](https://github.com/herdrdev/herdr) | `c411883ec639c9893ed9c33021c890485e91727b` | Apache-2.0, `LICENSE`, SHA-256 `c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4` | Apache-2.0 |
| Luvus | [RizRiyz/luvus](https://github.com/RizRiyz/luvus) | `63cb48263f347093f952dc43bbe0c2d468609567` | Apache-2.0, `LICENSE`, SHA-256 `c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4` | Apache-2.0 |
| Orca ADE | [stablyai/orca](https://github.com/stablyai/orca) | `2c2414be5780ea6bf4cf058e8f4dda48d0d0d3b1` | MIT, `LICENSE`, SHA-256 `ff1b611f80580d49f4b97e93a97b24eb050b0671b26b8afe16341fab699112f3` | MIT |

The earlier Luvus README/LICENSE conflict is **not present at the pinned
commit above**: both files at `63cb482…` state Apache-2.0. This conclusion is
revision-specific; it does not reinterpret older revisions or releases.
Herdr's older AGPL release boundary also does not change the license of this
current pinned commit. Only source from the recorded revisions can be considered
for reuse; changing a pin requires rerunning this audit.

## Detected nested license and attribution files

SHA-256 digests below identify the exact files in the pinned tree. A root
license does not replace a nested third-party license or notice.

### Herdr

| Path | SHA-256 | First-line identification |
|---|---|---|
| `packaging/windows/licenses/Microsoft.Windows.Console.ConPTY-LICENSE.txt` | `5d177f23ecfeb0ea8e050b6a5a16355e1ae9a0b286436ca8f83ed08b3795be6b` | Microsoft copyright |
| `packaging/windows/licenses/Microsoft.Windows.Console.ConPTY-NOTICE.md` | `e7fbaadee6ab20c28b87730a510ee5f5815d8fb4bd88d1d54d282dc2a74c0726` | Windows Console ConPTY notices |
| `vendor/libghostty-vt/LICENSE` | `386211873e5b7a02f663ae4d7adf96285999f91608f8f9f31fecfd0f4095e6f1` | MIT License |
| `vendor/libghostty-vt/pkg/afl++/LICENSE` | `337a40d58de145036691382b3aec220ee28f507fe4f0db8970b6844f6dd60eb3` | zig-afl-kit-derived AFL material |
| `vendor/portable-pty/LICENSE.md` | `191c46fcf52061382b1c51a70311eb9081381cc158e5899f3739473a9432185b` | MIT License |

### Luvus

| Path | SHA-256 | First-line identification |
|---|---|---|
| `src/theme/paper/NOTICE` | `652490143a19dc5c9b2290ea290717dd5a47caa0da1e91643bb19b5f21f32128` | Paper palette adaptation attribution |
| `src/theme/papercolor/LICENSE` | `e337357fa410de84aa3f35490af0b2d0541ac44774876f7056140158a15f924e` | MIT License |
| `src/theme/rose-pine/LICENSE` | `fb2535bcb42729f0547691eea7feee12d6870d184d704a7a929513973974d7bd` | MIT License |
| `src/theme/tokyo-night/LICENSE` | `358ef84dd5b689c6c6d96cb0630661e61cf20b88749694905457230d56f57b0b` | MIT License |
| `vendor/alacritty_terminal/LICENSE-APACHE` | `8884289e3969c089cd556c53fa1d3f51d86b29e52a8c1d660a55ce8292986910` | Apache-2.0 |
| `vendor/vte/LICENSE-APACHE` | `62c7a1e35f56406896d7aa7ca52d0cc0d272ac022b5d2796e7d6905db8a3636a` | Apache-2.0 |
| `vendor/vte/LICENSE-MIT` | `e4c9b06fa850cb9b540a5e400e9f6394cf15efcf4098144de477d1d3dae10150` | MIT license |

### Orca ADE

| Path | SHA-256 | First-line identification |
|---|---|---|
| `docs/site/THIRD_PARTY_NOTICES.md` | `1e039dc5567ff96b755c80a384dd626a00a0c46c49e9994c86cb599efcc435fd` | Third-party notices |
| `mobile/packages/expo-two-way-audio/LICENSE` | `bc32465e1384fbea714ddf43e4849aae2e3662ab8a7c8c4a35a1bf41c7b5ad81` | MIT License |

The vendored `pkg/afl++/LICENSE` text, every attribution entry, repository
assets/fonts/themes, and all lockfile-resolved dependency licenses still need a
file-level disposition before corresponding material is reused. This table is
a detected root/nested notice inventory, not a complete dependency SBOM.

## Herdr manifest data bundled by Rover

These TOML files are byte-identical to `src/detect/manifests/` at the pinned
Herdr commit in the immutable revision table. Rover includes them under
`crates/rover-agents/src/herdr_manifests/`; their behavior is evaluated by
Rover's separate Rust implementation. The source commit root declares
Apache-2.0. Rover retains that exact `LICENSE` at
`licenses/upstream/herdr/LICENSE` (SHA-256
`c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4`).
No manifest contains a separate copyright or license header.

| Pinned source path | Rover path | SHA-256 |
|---|---|---|
| `src/detect/manifests/amp.toml` | `crates/rover-agents/src/herdr_manifests/amp.toml` | `b5806b0dd21e2f5e752d0eac7f084d5ce2e3f275638cfd1234c42cc88152dc8c` |
| `src/detect/manifests/antigravity.toml` | `crates/rover-agents/src/herdr_manifests/antigravity.toml` | `11300b853130d037eb2c57d9c4b897893cc1f0a876e12eb54ff1b216177db9d7` |
| `src/detect/manifests/claude.toml` | `crates/rover-agents/src/herdr_manifests/claude.toml` | `038d0aa23fee3f9b39cb3c9ca117d0f95b0b3a5873cf0f38284ccbac279c9664` |
| `src/detect/manifests/cline.toml` | `crates/rover-agents/src/herdr_manifests/cline.toml` | `75fe33ec735c59638da8d62e16bddd257d9959e692edb83b1f11b7f28057866a` |
| `src/detect/manifests/codex.toml` | `crates/rover-agents/src/herdr_manifests/codex.toml` | `aba3d44a0f7177d5dd9fd60002a8efd11ef6e1a589bb3992c02563ed3e0de5e0` |
| `src/detect/manifests/cursor.toml` | `crates/rover-agents/src/herdr_manifests/cursor.toml` | `753b1f7f632d42fa21139c2767ecbb5e1078748aba2e59407ac4932d3ce36ad7` |
| `src/detect/manifests/devin.toml` | `crates/rover-agents/src/herdr_manifests/devin.toml` | `250c9cea1d60bdb965dc6056f3066b785e941d60242756fbaca73b57ca6b0f85` |
| `src/detect/manifests/droid.toml` | `crates/rover-agents/src/herdr_manifests/droid.toml` | `d37e7c464177c0e8f3edce8d4fabc4bcc7a1874edf2c4c87a2f927888cf69ce9` |
| `src/detect/manifests/gemini.toml` | `crates/rover-agents/src/herdr_manifests/gemini.toml` | `d7013b5e772852ecc595febf964f00b4f9edcbc6047a2a5421613a154d92520d` |
| `src/detect/manifests/github-copilot.toml` | `crates/rover-agents/src/herdr_manifests/github-copilot.toml` | `b70c652584326a1a98475a5fcef16207dee9f23b65f8e78300f4fc2bb578cb11` |
| `src/detect/manifests/grok.toml` | `crates/rover-agents/src/herdr_manifests/grok.toml` | `4298dd2ea7d124f3ba5d72b649bbb4fb6edc533d7e6e546d523a8214fa31456d` |
| `src/detect/manifests/hermes.toml` | `crates/rover-agents/src/herdr_manifests/hermes.toml` | `533d21b65dea3a0c60c25d0475c9c900d28a5712b6392669a7788f75de2b6e85` |
| `src/detect/manifests/kilo.toml` | `crates/rover-agents/src/herdr_manifests/kilo.toml` | `70f0ba4e58bc141fe69d7024013f973cd16e8616393079318914d70afeefef3b` |
| `src/detect/manifests/kimi.toml` | `crates/rover-agents/src/herdr_manifests/kimi.toml` | `ede08c0d2d5024f7606dc0a1b2f7a9c6a0ebb99f3b6c58ca6757049856d06e05` |
| `src/detect/manifests/kiro.toml` | `crates/rover-agents/src/herdr_manifests/kiro.toml` | `c8990f3c9d4810995be97e8df29836411e37d90f6ac9d9303f505787b504806b` |
| `src/detect/manifests/letta.toml` | `crates/rover-agents/src/herdr_manifests/letta.toml` | `205b8c135584c86f9f529aa2d061109b5129085cc5c3124c6dbfe0e78fcdba43` |
| `src/detect/manifests/maki.toml` | `crates/rover-agents/src/herdr_manifests/maki.toml` | `3b392170ee3082051266509f575a4640bde8d693b2f37ecf52157a641bc75b28` |
| `src/detect/manifests/muse.toml` | `crates/rover-agents/src/herdr_manifests/muse.toml` | `b69c4d87fa9c19e3e6453b706fbe39c98a8b33ffbaa48e8cd5ae6751e9615074` |
| `src/detect/manifests/opencode.toml` | `crates/rover-agents/src/herdr_manifests/opencode.toml` | `faa82aed2d76ad856528caae04232894594b74cc903aed3e522b3e725c5f2c95` |
| `src/detect/manifests/pi.toml` | `crates/rover-agents/src/herdr_manifests/pi.toml` | `57469b82e4239bf93559ed5d400c9d9f86a9e64d5def728b7d4bb398be7659ca` |
| `src/detect/manifests/qodercli.toml` | `crates/rover-agents/src/herdr_manifests/qodercli.toml` | `2089f70fc78c6576fd7128a00f4e7eedb85be81bde9ed73301b3b88000048961` |
| `src/detect/manifests/qwen.toml` | `crates/rover-agents/src/herdr_manifests/qwen.toml` | `b27aa456af228e8a4ceac74f0dd431c33b21473b69314fc672a7fba474e2a7fe` |

## Pinned source-to-capability roots

Paths are relative to the pinned commit above. These are the implementation
areas to inspect for behavior. They are not blanket approval to copy every file.

| Upstream | CLI/protocol | Session/terminal | Agents/orchestration | Git/remote/files | Extensions/settings |
|---|---|---|---|---|---|
| Herdr | `src/cli/`, `src/api/`, `src/protocol/` | `src/server/`, `src/client/`, `src/workspace/`, `src/pane/`, `src/terminal/`, `src/pty/`, `src/persist/` | `src/detect/`, `src/integration/`, `src/agent_resume.rs`, `src/agent_view_eval.rs` | `src/remote/`, `src/workspace/` | `src/plugin_command.rs`, `src/plugin_paths.rs`, `src/config/`, `src/ui/`, `src/input/` |
| Luvus | `src/app/`, `src/ipc/`, `src/api/`, `src/uhp/` | `src/terminal/`, `src/platform/`, `src/ui/` | `src/agent/`, `src/mission/`, `src/orch/`, `src/automation/` | `src/git/`, `src/machine/`, `src/files/`, `src/search/`, `src/diff/` | `src/module/`, `src/theme/`, `src/i18n/`, `src/bar/` |
| Orca ADE | `src/cli/`, `src/shared/rpc-contract/` | `src/main/daemon/`, `src/main/pty/`, `src/main/ghostty/`, `src/renderer/src/components/terminal/` | `src/main/agent-launch/`, provider directories under `src/main/`, `src/main/automations/` | `src/main/git/`, `src/main/ssh/`, `src/main/browser/`, `src/main/ipc/worktrees/`, `src/renderer/src/components/editor/` | `src/main/plugins/`, `src/main/keybindings/`, `src/main/notifications/`, `src/shared/plugins/` |

Orca also has separate `mobile/`, `cloud/`, `native/`, and Electron renderer
areas. Their behavior is in the parity audit because the user requested all
features, but Rover's deliverable remains Rust CLI/TUI: port the behavior into
those surfaces; do not add an Orca-style GUI/mobile/cloud application to Rover.

## Reuse gate

User clarified: port all behavior into Rust CLI/TUI and reuse source only when
its exact path, digest, license, copyright/NOTICE, and relevant acceptance test
have been audited. The manifest data above is the first audited reuse; no
upstream application code has been copied into Rover. Before further reuse,
record the exact commit/path/blob digest, transformation notice, license
obligations, nested dependency/asset notices, and the test that proves the
resulting Rust behavior. Implement the capability independently if that
evidence is incomplete.

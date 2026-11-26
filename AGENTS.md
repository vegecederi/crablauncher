# Crab Launcher — Agent Guidelines

## 1. Project Philosophy

Crab is a fast, minimalist, native Minecraft launcher built with Rust and Slint.

The project prioritizes:

- Performance and responsiveness.
- Visual consistency.
- Reliability and functional correctness.
- Customization and flexibility.
- Maintainable and modular architecture.
- A clean, intuitive user experience.

Every implementation must respect these principles.

## 2. UI & Design Consistency

The Crab Launcher must maintain a consistent visual identity across all windows, dialogs, menus, and components.

- Reuse existing UI components instead of creating unnecessary variations.
- Maintain consistent padding, margins, spacing, alignment, typography, border radii, and control dimensions.
- Buttons, checkboxes, dropdowns, context menus, text fields, and scrollbars must follow the same design standards.
- Avoid unnecessary backgrounds, borders, visual clutter, and excessive highlighting.
- Ensure text and icons never overflow, overlap, or become incorrectly positioned.
- Layouts must adapt correctly when windows are resized.
- Containers must use the available space appropriately.
- Identical actions must use identical dialogs and behavior, regardless of where they are triggered.
- Preserve UI consistency when elements are repositioned by the user.
- Popups and dropdowns must be correctly anchored to their triggering controls.
- Use existing Lucide icons and the icon pack system whenever applicable.

Do not redesign unrelated UI elements unless explicitly requested.

## 3. Themes & Customization

All UI components must respect the active theme and icon pack.

- Never hardcode UI colors when theme tokens are available.
- Ensure new components work with both light and dark themes.
- Preserve compatibility with official, special, and community themes.
- Respect user preferences regarding layout, typography, animations, and appearance.
- Provide appropriate fallbacks for missing theme properties or icons.
- Theme and icon pack changes must update the interface immediately.
- Appearance settings must not negatively affect functionality.

## 4. Functional Correctness

**A feature is not complete simply because it compiles or appears in the interface.**

Every implemented feature must perform its intended operation correctly.

- Never implement decorative buttons or non-functional controls as completed features.
- Never silently ignore errors.
- Never report success before the underlying operation has actually succeeded.
- Validate the entire workflow, from user interaction to backend execution and persistence.
- Verify that changes remain effective after restarting the launcher.
- Ensure related UI elements remain synchronized.
- Handle invalid input, incompatible configurations, missing files, network failures, and unexpected process termination.

For example, Replace Minecraft.jar is only considered functional when Minecraft actually launches using the replacement JAR and its modifications are present in-game.

Successfully spawning a Java process does not prove that Minecraft launched successfully.

## 5. Settings & Interaction

Settings must follow the Click & Apply principle.

- Apply and persist valid settings immediately after user interaction.
- The OK button should close the settings window, not commit pending changes.
- Keep global and instance-specific settings synchronized according to their inheritance rules.
- Destructive or complex operations may require explicit confirmation.
- Keyboard shortcuts must execute the same actions as their corresponding buttons and context menu entries.
- Disabled controls must accurately reflect unavailable or incompatible operations.
- Never require unnecessary clicks to perform simple actions.

## 6. Performance

Performance is a core requirement, not an optional improvement.

- Never block the UI thread with network requests, filesystem operations, downloads, or expensive computations.
- Use asynchronous operations and background tasks where appropriate.
- Avoid unnecessary allocations, cloning, polling, and repeated computations.
- Cache frequently accessed metadata and images.
- Use lazy loading and pagination for large collections.
- Preserve smooth scrolling and responsive user interactions.
- Avoid unnecessary UI reconstruction and excessive model updates.
- Keep startup operations minimal and defer non-essential work.
- Measure performance before and after significant optimizations.

Evaluate actual application performance using release builds.

Do not introduce performance regressions when implementing new features.

## 7. Architecture & Code Quality

- Inspect existing implementations before making changes.
- Fix root causes instead of introducing temporary workarounds.
- Reuse existing services, components, utilities, and data models.
- Avoid duplicated logic and unnecessary dependencies.
- Keep UI, business logic, persistence, and external service integrations appropriately separated.
- Follow existing Rust and Slint conventions.
- Keep the project structure organized.
- Remove obsolete code introduced or replaced by the current implementation.
- Preserve existing public interfaces unless changes are necessary.

Do not rewrite entire systems to solve isolated problems.

## 8. Minecraft Compatibility

The launcher must support Minecraft versions and configurations accurately.

- Respect version-specific Java, LWJGL, asset, library, and launch requirements.
- Preserve compatibility with Alpha, Beta, legacy, and modern Minecraft versions.
- Validate modloader compatibility before installation.
- Never assume that modern launch behavior applies to legacy versions.
- Ensure custom JARs, component overrides, and launch arguments are actually used during execution.
- Preserve user files when changing versions, components, or modloaders.
- Report incompatibilities clearly instead of allowing silent failures.

## 9. Data Integrity & Error Handling

- Never delete user data without appropriate authorization.
- Protect worlds, instances, mods, accounts, and configuration files.
- Use safe and atomic persistence whenever possible.
- Clean up incomplete downloads and failed installations.
- Preserve existing working configurations when an operation fails.
- Capture relevant process output, errors, and exit codes.
- Never expose authentication tokens, API keys, or other credentials in logs or source code.

## 10. Testing & Validation

Before declaring a task complete, validate the actual functionality.

Run the appropriate checks:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

For UI changes:

- Verify the actual visual result.
- Test resizing and layout behavior.
- Test relevant light and dark themes.
- Verify mouse and keyboard interactions.
- Check for clipping, overflow, alignment issues, and inconsistent spacing.

For functional changes:

- Test the complete user workflow.
- Test successful and failed operations.
- Add regression tests when fixing bugs.
- Verify persistence when applicable.
- Ensure existing functionality has not been broken.

Compilation, automated tests, and startup smoke tests alone do not guarantee that a feature works correctly.

If manual or visual validation is unavailable, explicitly report that limitation.

## 11. Task Execution

- Follow every requirement in the user's request.
- Keep track of individual requirements throughout implementation.
- Do not silently skip requested features or corrections.
- Do not expand the task beyond its intended scope.
- Prioritize correctness over unnecessary complexity.
- Do not introduce unrelated changes.
- Never claim that an untested feature is fully functional.
- Clearly report incomplete implementations, limitations, and remaining issues.

## 12. Definition of Done

A task is complete only when:

- [ ] All requested requirements have been addressed.
- [ ] The implementation performs its intended operation.
- [ ] The UI follows Crab's existing design standards.
- [ ] Error handling is implemented.
- [ ] Settings and data persist correctly when applicable.
- [ ] Existing functionality remains intact.
- [ ] No known performance regressions were introduced.
- [ ] Relevant tests and validation checks pass.
- [ ] Any unverified behavior or remaining limitations are explicitly reported.

**Never confuse implementation with completion. A feature must work, not merely exist.**

## Git Commit Guidelines

- Always write short, clear and descriptive commit messages in English.
- Every commit message must begin with exactly one relevant emoji.
- Use concise Title Case messages.
- Avoid unnecessary prefixes, descriptions and verbose messages.
- Each commit should represent a coherent set of changes.
- Never commit credentials, API keys, personal data or build artifacts.
- Never push changes without explicit authorization.

Examples:

- 🦀 Initial Commit
- 🐛 Fix Account Dropdown
- ⚡ Improve Startup Performance
- 🎨 Refine Instance Settings
- ✨ Add Icon Packs
- ♻️ Refactor Download Manager
- 📝 Update README
- 🔧 Update Configuration
- 🔒 Improve Account Security
- 📦 Update Dependencies
- 🗑️ Remove Unused Components
- 🚀 Release v0.1.0

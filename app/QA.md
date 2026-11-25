# Manual QA

What the automated tests can't reach (SPEC-app "Testing"). Run it in the real app
(`pnpm tauri build`), keyboard only, in dark and in light, before each merge to main.

## The UI in the real web view

- [ ] The window opens on Needs me with no white flash; the first item is selected.
- [ ] Traffic lights sit in the sidebar's top 40px; the sidebar top and the 44px header drag the window; buttons in them still click.
- [ ] ⌘K, ⌘N, ⌘, and ⌘Z act from the page before the native menu (Not verified yet in SPEC-app); a single press never runs a command twice.
- [ ] In a text field, S, Space, arrows, ⌘↩ and ⌘⌫ edit the text; ⌘K and ⌘N still open Jump and New task.
- [ ] Task-menu items are disabled while a field or an overlay has focus, and "Undo <action>" relabels.
- [ ] System accent: focus rings, the keyboard outline and text selection follow it; Done, progress and the primary button stay indigo; Multicolor keeps indigo.
- [ ] Settings › Appearance: System, Dark and Light switch at once, and the window background follows.
- [ ] Reduce Motion and Increase Contrast (System Settings › Accessibility › Display) visibly apply.
- [ ] VoiceOver reads a row ("144, Web: slot picker, In progress, …"), a Needs me item and a map station; toasts are announced.
- [ ] Scroll All tasks with 2,000 tasks (`cargo run -p airboss-app --example seed -- --tasks 2000`): no dropped frames in Web Inspector's timeline.

## Quick add

- [ ] ⌃⌥N from another app (also over a full-screen app) opens the panel with the cursor in the title, under 100 ms.
- [ ] ↓ moves to notes, Tab opens repos, Shift-Tab opens parent; ↩ creates and the panel hides; ⌘↩ creates and starts Do it.
- [ ] Clicking elsewhere hides the panel and keeps a typed title; Esc clears it.
- [ ] A Do it failure from quick add shows the main window with the task selected and the toast.

## Screen 08: the menu-bar menu

The native menu can't be screenshotted by Playwright. Capture it with `screencapture -i` in a
light and a dark menu bar and compare it with `docs/design/screens/08-menu-bar.png`:

- [ ] The count is the title (nothing at 0); the accessibility label reads "Airboss, 8 waiting on you".
- [ ] "Needs me 8" (disabled), up to six items with their kind's red icon and clock times ("10:32", "Mon 10:32", "25 Nov").
- [ ] Choosing an item opens the main window on Needs me with it selected.
- [ ] "Open Airboss ⌘0" and "Quick Add… ⌃⌥N", then "Quit Airboss"; empty: "Nothing needs you".

## Links out

- [ ] Open thread opens the right thread in T3 Code (`t3code://threads/<environment>/<thread>`).
- [ ] Open PR and ⌘O open the browser; Show in Finder (database error) opens the folder.

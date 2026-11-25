# Airboss design

The rules are in [DESIGN.md](../../DESIGN.md). This folder holds the material behind them.

| What | Where |
|---|---|
| Working prototype: click it, use it from the keyboard, switch dark and light | [prototype.html](prototype.html), which opens on its own in a browser |
| Tokens and component styles (dark first) | [airboss.css](airboss.css) |
| Screens exported from the Paper file "Airboss" | [screens/](screens/) |

The Paper file has four pages: Foundations, App screens, Overlays and States. It uses the same tokens as `airboss.css`. All task names, numbers and repos in the designs are made-up sample data.

## Screens

| | |
|---|---|
| ![All tasks with the detail panel](screens/01-all-tasks-detail.png) | ![Needs me](screens/02-needs-me.png) |
| All tasks, with the selected task in the detail panel | Needs me: decisions in a two-pane inbox |
| ![Task page with run order](screens/03-task-page-run-order.png) | ![Plans](screens/04-plans.png) |
| Task page: progress and the run-order map | Plans: every parent task with its progress |
| ![New task](screens/05-new-task.png) | ![Jump](screens/06-jump.png) |
| New task (⌃⌥N from anywhere, C or ⌥N in the app) | Jump to a task or run a command (⌘K) |
| ![Status and right-click menus](screens/07-status-and-context-menus.png) | ![Menu bar](screens/08-menu-bar.png) |
| Status picker (S) and right-click menu | Menu-bar icon and dropdown |
| ![Shortcuts](screens/09-shortcuts.png) | ![First run](screens/10-first-run.png) |
| Keyboard shortcuts (?) | First run, with setup checks |
| ![Nothing needs you](screens/11-nothing-needs-you.png) | ![T3 Code missing](screens/12-t3-missing.png) |
| Nothing needs you | T3 Code missing: Do it and Plan it switch off |
| ![Light mode](screens/13-light-mode.png) | |
| Light mode | |

## Foundations

![Colour](screens/f1-colour.png)
![Type](screens/f2-type.png)
![Status and progress](screens/f3-status-progress.png)
![Components](screens/f4-components.png)

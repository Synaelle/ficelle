# @app/ui

Shared design layer.

- `themes/`: the built-in theme as JSON, its schema and the list in `index.ts`.
- `src/theme.ts`: theme validation, light or dark resolution, accent override, conversion to css variables.
- `src/color.ts`: contrast math.
- `brand/app-icon.svg`: the knot, source of every platform icon. Run `pnpm icons` after editing it. The `-dev` and `-preview` variants mark dev and preview builds.

React 19: `ref` is a regular prop of function components; `forwardRef` is not needed.
Form actions use `useActionState` from `react` (not `useFormState` from `react-dom`); `act` comes from `react`, not `react-dom/test-utils`.
Function components have no `propTypes` or `defaultProps`: use TypeScript types and default parameter values; an object, array or function default is hoisted to a module constant.
`ReactDOM.render` is removed: use `createRoot` from `react-dom/client`.
Components pass React Compiler's checks (Biome `useReactCompiler`): no `setState` called synchronously in an effect, no ref read or written while rendering, no `Date.now()` / `Math.random()` while rendering; props are never assigned to.

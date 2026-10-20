React 19: `ref` is a regular prop of function components; `forwardRef` is not needed.
Form actions use `useActionState` from `react` (not `useFormState` from `react-dom`); `act` comes from `react`.
Function components have no `propTypes` or `defaultProps`: use TypeScript types and default parameter values.
`ReactDOM.render` is removed: use `createRoot` from `react-dom/client`.

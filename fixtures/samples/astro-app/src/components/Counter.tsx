import { useState } from "react";

export default function Counter({ start }: { start: number }) {
  const [n, setN] = useState(start);
  return <button type="button" onClick={() => setN(n + 1)}>{n}</button>;
}

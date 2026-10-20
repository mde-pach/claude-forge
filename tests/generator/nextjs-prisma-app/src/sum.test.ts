import { expect, it } from "vitest";
import { sum } from "./sum";

it("adds", () => {
  expect(sum(1, 2)).toBe(3);
});

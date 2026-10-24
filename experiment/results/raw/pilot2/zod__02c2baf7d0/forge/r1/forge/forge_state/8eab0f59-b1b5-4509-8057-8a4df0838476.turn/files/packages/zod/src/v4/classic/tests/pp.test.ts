import { expect, expectTypeOf, test } from "vitest";
import * as z from "zod/v4";

test("pp", () => {
  const f = (v: unknown) => v;
  const o = z.object({ x: z.preprocess(f, z.number().optional()), y: z.preprocess(f, z.number()).optional() });
  expect(o.safeParse({}).success).toBe(true);
  const p = z.preprocess(f, z.string());
  expect(p instanceof z.ZodPipe && p instanceof z.ZodPreprocess).toBe(true);
  expectTypeOf(p).toMatchTypeOf<z.ZodPipe<z.core.$ZodTransform, z.ZodString>>();
  expectTypeOf<z.input<typeof p>>().toEqualTypeOf<unknown>();
  const r = z.record(
    z.preprocess((v) => String(v).toLowerCase(), z.enum(["a", "b"])),
    z.string()
  );
  expect(r.parse({ A: "x", B: "y" })).toEqual({ a: "x", b: "y" });
  const c = z.codec(z.string(), z.number(), { decode: Number, encode: String }).meta({ examples: [42] });
  expect(z.toJSONSchema(c, { io: "input" })).not.toHaveProperty("examples");
  expect(z.toJSONSchema(c, { io: "output" })).toHaveProperty("examples");
});

"use client";

export function serialize(value: unknown): string {
  return JSON.stringify(
    value,
    (_, value) => (typeof value === "bigint" ? value.toString() : value),
    2
  );
}

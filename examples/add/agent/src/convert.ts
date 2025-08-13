import { Provable, Field, assert, Bool } from "o1js";
import { Fr, CanonicalElement } from "@silvana-one/mina-utils";

export function convertFieldToCanonicalElement(field: Field): CanonicalElement {
  const bitsField = field.toBits();
  assert(bitsField.length === 254, "Field has to be 254 bits");
  const element: CanonicalElement = Provable.witness(
    Fr.Canonical.provable,
    () => {
      return Fr.from(field.toBigInt());
    }
  );

  const bitsElement = element.toBits();
  assert(bitsElement.length === 255, "Element has to be 255 bits");
  bitsElement[254].assertEquals(Bool(false));
  for (let i = 0; i < 254; i++) {
    bitsElement[i].assertEquals(bitsField[i]);
  }
  return element;
}

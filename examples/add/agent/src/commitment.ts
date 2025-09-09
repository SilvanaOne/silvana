import { Field, UInt32, UInt64, Struct, Encoding, Poseidon } from "o1js";
import {
  Fr,
  update,
  digestStruct,
  scalar,
  R,
  convertFieldToCanonicalElement,
} from "@silvana-one/mina-utils";

const ADD_SCALAR = scalar(Encoding.stringToFields("add")[0].toBigInt());
const MULTIPLY_SCALAR = scalar(
  Encoding.stringToFields("multiply")[0].toBigInt()
);
const SUM_INDEX = UInt32.from(0);

export class AddProgramCommitment extends Struct({
  stateCommitment: Fr.Canonical.provable,
  actionsCommitment: Fr.Canonical.provable,
  actionsSequence: UInt64,
  actionsRPower: Fr.Canonical.provable,
}) {
  static assertEquals(a: AddProgramCommitment, b: AddProgramCommitment) {
    a.stateCommitment.assertEquals(b.stateCommitment);
    a.actionsCommitment.assertEquals(b.actionsCommitment);
    a.actionsSequence.assertEquals(b.actionsSequence);
    a.actionsRPower.assertEquals(b.actionsRPower);
  }

  serialize(): string {
    return JSON.stringify({
      stateCommitment: this.stateCommitment.toBigInt().toString(),
      actionsCommitment: this.actionsCommitment.toBigInt().toString(),
      actionsSequence: this.actionsSequence.toBigInt().toString(),
      actionsRPower: this.actionsRPower.toBigInt().toString(),
    });
  }

  static deserialize(str: string): AddProgramCommitment {
    const {
      stateCommitment,
      actionsCommitment,
      actionsSequence,
      actionsRPower,
    } = JSON.parse(str);
    return new AddProgramCommitment({
      stateCommitment: Fr.from(BigInt(stateCommitment)).assertCanonical(),
      actionsCommitment: Fr.from(BigInt(actionsCommitment)).assertCanonical(),
      actionsSequence: UInt64.from(BigInt(actionsSequence)),
      actionsRPower: Fr.from(BigInt(actionsRPower)).assertCanonical(),
    });
  }

  hash(): Field {
    return Poseidon.hashPacked(AddProgramCommitment, this);
  }
}

export function calculateNewCommitment(params: {
  methodName: "add" | "multiply";
  inputCommitment: AddProgramCommitment;
  inputMoveCommitment: AddProgramCommitment;
  outputMoveCommitment: AddProgramCommitment;
  index: UInt32;
  value: Field;
  old_value: Field;
  new_value: Field;
  old_sum: Field;
  new_sum: Field;
}): AddProgramCommitment {
  const {
    methodName,
    inputCommitment,
    inputMoveCommitment,
    outputMoveCommitment,
    index,
    value,
    old_value,
    new_value,
    old_sum,
    new_sum,
  } = params;
  AddProgramCommitment.assertEquals(inputCommitment, inputMoveCommitment);

  const newStateCommitmentIndex = update(
    inputCommitment.stateCommitment,
    digestStruct([convertFieldToCanonicalElement(old_value)]),
    digestStruct([convertFieldToCanonicalElement(new_value)]),
    index
  );
  const outputStateCommitment = update(
    newStateCommitmentIndex,
    digestStruct([convertFieldToCanonicalElement(old_sum)]),
    digestStruct([convertFieldToCanonicalElement(new_sum)]),
    SUM_INDEX
  );

  const actionCommitment = digestStruct([
    methodName === "add" ? ADD_SCALAR : MULTIPLY_SCALAR,
    convertFieldToCanonicalElement(index.value),
    convertFieldToCanonicalElement(value),
  ]);

  const actionsCommitment = inputCommitment.actionsCommitment
    .add(actionCommitment.mul(inputCommitment.actionsRPower).assertCanonical())
    .assertCanonical();

  const outputCommitment = new AddProgramCommitment({
    stateCommitment: outputStateCommitment,
    actionsCommitment,
    actionsSequence: inputCommitment.actionsSequence.add(1),
    actionsRPower: inputCommitment.actionsRPower.mul(R).assertCanonical(),
  });

  AddProgramCommitment.assertEquals(outputCommitment, outputMoveCommitment);
  return outputCommitment;
}

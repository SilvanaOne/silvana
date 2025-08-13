import { describe, it } from "node:test";
import assert from "node:assert";
import { UInt32, Encoding } from "o1js";
import {
  scalar,
  R,
  digestStruct,
  update,
  rScalarPow,
} from "@silvana-one/mina-utils";
/*
add result {
  index: 1,
  value: 1n,
  new_sum: 1n,
  new_value: 1n,
  old_sum: 0n,
  old_value: 0n,
  old_actions_commitment: 6248033897n,
  new_actions_commitment: 11342841017146411466436790246763261178205876912743807303577698841268502875532n,
  old_state_commitment: 0n,
  new_state_commitment: 9328350379599323964930814050805468085812740822563546972518289385183440774483n
}
*/

const R2 = R.mul(R).assertCanonical();
const R3 = R2.mul(R).assertCanonical();

describe("Test commitment", async () => {
  it("should calculate state commitment", async () => {
    // create state commitment
    const createdStateCommitment = scalar(0n);
    const structCommitment0v0 = digestStruct([scalar(0n)]);
    const stateCommitmentv0 = structCommitment0v0;
    // add 1
    const structCommitment0v1 = digestStruct([scalar(1n)]);
    const structCommitment1v1 = digestStruct([scalar(1n)]);
    const stateCommitmentv1 = structCommitment1v1
      .mul(R)
      .add(structCommitment0v1);
    const updatedStateCommitmentv1temp = update(
      stateCommitmentv0,
      scalar(0n),
      structCommitment1v1,
      UInt32.from(1)
    );
    const updatedStateCommitmentv1 = update(
      updatedStateCommitmentv1temp,
      structCommitment0v0,
      structCommitment0v1,
      UInt32.from(0)
    );
    assert.strictEqual(
      updatedStateCommitmentv1.toBigInt(),
      9328350379599323964930814050805468085812740822563546972518289385183440774483n
    );
  });
  /*
add result {
  index: 1,
  value: 1n,
  new_sum: 1n,
  new_value: 1n,
  old_sum: 0n,
  old_value: 0n,
  old_actions_commitment: 6248033897n,
  new_actions_commitment: 41214508720617712057645573797159612427662368158173771695682160867969771078564n,
  old_state_commitment: 0n,
  new_state_commitment: 9328350379599323964930814050805468085812740822563546972518289385183440774483n,
  old_actions_sequence: 1n,
  new_actions_sequence: 2n
}
  */
  it("should calculate actions commitment", async () => {
    const createdActionsCommitment = scalar(0n);
    const createdActionsPower = scalar(1n);
    const initField = Encoding.stringToFields("init")[0].toBigInt();
    const initialActionsCommitment = createdActionsCommitment.add(
      createdActionsPower.mul(scalar(initField))
    );
    assert.strictEqual(initialActionsCommitment.toBigInt(), 6248033897n);
    const addActionsPower = createdActionsPower.mul(R).assertCanonical();
    const addField = Encoding.stringToFields("add")[0].toBigInt();
    const actionCommitment = digestStruct([
      scalar(addField),
      scalar(1n),
      scalar(1n),
    ]);
    const addActionsCommitment = initialActionsCommitment.add(
      addActionsPower.mul(actionCommitment)
    );
    assert.strictEqual(
      addActionsCommitment.toBigInt(),
      41214508720617712057645573797159612427662368158173771695682160867969771078564n
    );
  });
});

import {
  Field,
  AccountUpdate,
  UInt64,
  PublicKey,
  state,
  State,
  SmartContract,
  method,
  Bool,
  DeployArgs,
  Permissions,
} from "o1js";
import { AddProgramState, AddMap, AddProgramProof } from "./circuit.js";

const initialState = AddProgramState.create().state;

interface AddContractDeployProps extends Exclude<DeployArgs, undefined> {
  admin: PublicKey;
  uri: string;
}

export class AddContract extends SmartContract {
  @state(PublicKey) admin = State<PublicKey>();
  @state(Field) root = State<Field>(Field(0));
  @state(Field) length = State<Field>(Field(0));
  @state(Field) sum = State<Field>(Field(0));
  @state(UInt64) sequence = State<UInt64>(UInt64.from(0));
  @state(UInt64) blockNumber = State<UInt64>(UInt64.from(0));
  @state(Field) commitmentHash = State<Field>(Field(0));

  /**
   * Deploys the contract with initial settings.
   * @param props - Deployment properties including admin and uri.
   */
  async deploy(props: AddContractDeployProps) {
    await super.deploy(props);
    this.admin.set(props.admin);
    this.root.set(initialState.root);
    this.length.set(initialState.length);
    this.sum.set(initialState.sum);
    this.sequence.set(UInt64.zero);
    this.blockNumber.set(UInt64.zero);
    this.commitmentHash.set(initialState.commitment.hash());

    this.account.zkappUri.set(props.uri);
    this.account.permissions.set({
      ...Permissions.default(),
      // Allow the upgrade authority to set the verification key
      // even when there is no protocol upgrade
      setVerificationKey:
        Permissions.VerificationKey.proofDuringCurrentVersion(),
      setPermissions: Permissions.impossible(),
      access: Permissions.proof(),
      send: Permissions.proof(),
      setZkappUri: Permissions.proof(),
      setTokenSymbol: Permissions.proof(),
    });
  }

  events = {
    settle: AddProgramState,
  };

  @method async settle(proof: AddProgramProof) {
    // verify the proof
    proof.verify();
    proof.publicInput.blockNumber.assertEquals(proof.publicOutput.blockNumber);

    // verify the sender is the admin
    const sender = this.sender.getUnconstrained();
    const senderUpdate = AccountUpdate.createSigned(sender);
    senderUpdate.body.useFullCommitment = Bool(true);
    this.admin.requireEquals(sender);

    // Verify the proof input matches current contract state
    this.blockNumber.requireEquals(
      proof.publicInput.blockNumber.sub(UInt64.from(1))
    );
    this.sequence.requireEquals(proof.publicInput.sequence);
    this.sum.requireEquals(proof.publicInput.sum);
    this.root.requireEquals(proof.publicInput.root);
    this.length.requireEquals(proof.publicInput.length);

    // Verify commitment hash matches
    this.commitmentHash.requireEquals(proof.publicInput.commitment.hash());

    // Update contract state with proof output
    this.sequence.set(proof.publicOutput.sequence);
    this.sum.set(proof.publicOutput.sum);
    this.root.set(proof.publicOutput.root);
    this.length.set(proof.publicOutput.length);
    this.blockNumber.set(proof.publicOutput.blockNumber);

    // Update commitment hash
    this.commitmentHash.set(proof.publicOutput.commitment.hash());

    this.emitEvent("settle", proof.publicOutput);
  }
}

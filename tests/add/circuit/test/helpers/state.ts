import {
  fetchSuiDynamicFieldsList,
  fetchSuiDynamicField,
  fetchSuiObject,
  suiClient,
} from "@silvana-one/coordination";

export async function getState(
  params: { appID?: string } = {}
): Promise<bigint[]> {
  const { appID = process.env.APP_OBJECT_ID } = params;
  if (!appID) {
    throw new Error("APP_OBJECT_ID is not set");
  }
  const object = await fetchSuiObject(appID);
  if (object?.data?.content?.dataType !== "moveObject")
    throw new Error("Object not found");
  const stateObjectID = (object?.data?.content?.fields as any).instance.fields
    .state.fields.state.fields.id.id;
  const state: bigint[] = [];
  const fields = await fetchSuiDynamicFieldsList(stateObjectID);
  const names = fields.data.map((field) => field.name);

  for (const name of names) {
    const element = await suiClient.getDynamicFieldObject({
      parentId: stateObjectID,
      name,
    });
    if (element.data?.content?.dataType !== "moveObject")
      throw new Error("Element not found");
    const value = BigInt((element.data?.content.fields as any).state[0]);
    state.push(value);
  }
  return state;
}

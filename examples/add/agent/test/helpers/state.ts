import {
  fetchSuiDynamicFieldsList,
  fetchSuiDynamicField,
  fetchSuiObject,
  suiClient,
} from "@silvana-one/coordination";

export async function getState(
  params: { appInstanceID?: string } = {}
): Promise<bigint[]> {
  const { appInstanceID = process.env.APP_INSTANCE_ID } = params;
  if (!appInstanceID) {
    throw new Error("APP_INSTANCE_ID is not set");
  }
  // Get the AppInstance object
  const appInstance = await fetchSuiObject(appInstanceID);
  if (appInstance?.data?.content?.dataType !== "moveObject")
    throw new Error("AppInstance not found");
  
  // The state is inside the AppInstance
  const stateObjectID = (appInstance?.data?.content?.fields as any).state.fields
    .state.fields.id.id;
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

# GetDsoInfoResponse

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**sv_user** | **String** | User ID representing the SV | 
**sv_party_id** | **String** | Party representing the SV | 
**dso_party_id** | **String** | Party representing the whole DSO; for Scan only, also returned by `/v0/dso-party-id`  | 
**voting_threshold** | **i32** | Threshold required to pass vote requests; also known as the \"governance threshold\", it is always derived from the number of `svs` in `dso_rules`  | 
**latest_mining_round** | [**models::ContractWithState**](ContractWithState.md) |  | 
**amulet_rules** | [**models::ContractWithState**](ContractWithState.md) |  | 
**dso_rules** | [**models::ContractWithState**](ContractWithState.md) |  | 
**sv_node_states** | [**Vec<models::ContractWithState>**](ContractWithState.md) | For every one of `svs` listed in `dso_rules`, a contract of the Daml template `Splice.DSO.SvState.SvNodeState`. This does not include states for offboarded SVs, though they may still have an on-ledger state contract  | 
**initial_round** | Option<**String**> | Initial round from which the network bootstraps  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



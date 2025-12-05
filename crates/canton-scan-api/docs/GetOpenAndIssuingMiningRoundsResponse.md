# GetOpenAndIssuingMiningRoundsResponse

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**time_to_live_in_microseconds** | **i32** | Suggested cache TTL for the response; this should expire before the `opensAt` of any open rounds that may not be in this response yet.  | 
**open_mining_rounds** | [**std::collections::HashMap<String, models::MaybeCachedContractWithState>**](MaybeCachedContractWithState.md) | Always created with respect to an input set of contract IDs. If an input contract ID is absent from the keys of this map, that contract should be considered removed by the caller; if present, `contract` may be empty, reflecting that the caller should already have the full contract data for that contract ID. Contracts not present in the input set will have full contract data. `domain_id` is always up-to-date; if undefined the contract is currently unassigned to a synchronizer, i.e. \"in-flight\".  | 
**issuing_mining_rounds** | [**std::collections::HashMap<String, models::MaybeCachedContractWithState>**](MaybeCachedContractWithState.md) | Always created with respect to an input set of contract IDs. If an input contract ID is absent from the keys of this map, that contract should be considered removed by the caller; if present, `contract` may be empty, reflecting that the caller should already have the full contract data for that contract ID. Contracts not present in the input set will have full contract data. `domain_id` is always up-to-date; if undefined the contract is currently unassigned to a synchronizer, i.e. \"in-flight\".  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



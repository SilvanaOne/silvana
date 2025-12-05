# EventHistoryVerdict

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**update_id** | **String** | The ID of the transaction update associated with this verdict.  | 
**migration_id** | **i64** | The migration id of the domain through which this event was sequenced.  | 
**domain_id** | **String** | The id of the domain through which this event was sequenced.  | 
**record_time** | **String** | The record_time of the transaction the verdict corresponds to.  | 
**finalization_time** | **String** | The finalization_time of the transaction the verdict corresponds to. Note that this time might be different between different scans/mediators.  | 
**submitting_parties** | **Vec<String>** | Parties on whose behalf the transaction was submitted.  | 
**submitting_participant_uid** | **String** | UID of the submitting participant.  | 
**verdict_result** | [**models::VerdictResult**](VerdictResult.md) |  | 
**mediator_group** | **i32** | The mediator group which finalized this verdict.  | 
**transaction_views** | [**models::TransactionViews**](TransactionViews.md) |  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



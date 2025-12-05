# UpdateHistoryTransactionV2

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**update_id** | **String** | The id of the update.  | 
**migration_id** | **i64** | The migration id of the synchronizer.  | 
**workflow_id** | **String** | This transaction's Daml workflow ID; a workflow ID can be associated with multiple transactions. If empty, no workflow ID was set.  | 
**record_time** | **String** | The time at which the transaction was sequenced, with microsecond resolution, using ISO-8601 representation.  | 
**synchronizer_id** | **String** | The id of the synchronizer through which this transaction was sequenced.  | 
**effective_at** | **String** | Ledger effective time, using ISO-8601 representation. This is the time returned by `getTime` for all Daml executed as part of this transaction, both by the submitting participant and all confirming participants.  | 
**root_event_ids** | **Vec<String>** | Roots of the transaction tree. These are guaranteed to occur as keys of the `events_by_id` object.  | 
**events_by_id** | [**std::collections::HashMap<String, models::TreeEvent>**](TreeEvent.md) | Changes to the ledger that were caused by this transaction, keyed by ID and sorted lexicographically by ID for display consistency. Values are nodes of the transaction tree. Within a transaction, IDs may be referenced by `root_event_ids` or `child_event_ids` in `ExercisedEvent` herein, which are sorted in the order as they occurred in the transaction.  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



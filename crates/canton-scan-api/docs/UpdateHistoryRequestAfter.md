# UpdateHistoryRequestAfter

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**after_migration_id** | **i64** | The migration id from which to start returning transactions. This is inclusive.  | 
**after_record_time** | **String** | The record time to start returning transactions from. This only affects transactions with the same migration id as after_migration_id. Higher migration ids are always considered to be later.  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



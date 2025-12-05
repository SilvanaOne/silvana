# GetMigrationInfoResponse

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**previous_migration_id** | Option<**i64**> | The migration id that was active before the given migration id, if any.  | [optional]
**record_time_range** | [**Vec<models::RecordTimeRange>**](RecordTimeRange.md) | All domains for which there are updates in the given migration id, along with the record time of the newest and oldest update associated with each domain  | 
**last_import_update_id** | Option<**String**> | The update id of the last import update (where import updates are sorted by update id, ascending) for the given migration id, if any  | [optional]
**complete** | **bool** | True if this scan has all non-import updates for given migration id  | 
**import_updates_complete** | Option<**bool**> | True if this scan has all import updates for the given migration id  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



# UpdateHistoryRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**after** | Option<[**models::UpdateHistoryRequestAfter**](UpdateHistoryRequestAfter.md)> |  | [optional]
**page_size** | **i32** | The maximum number of transactions returned for this request.  | 
**lossless** | Option<**bool**> | Whether contract payload should be encoded into json using a lossless, but much harder to process, encoding. This is mostly used for backend calls, and is not recommended for external users. Optional and defaults to false.  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



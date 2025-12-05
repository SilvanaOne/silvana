# Filters

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**cumulative** | Option<[**Vec<models::CumulativeFilter>**](CumulativeFilter.md)> | Every filter in the cumulative list expands the scope of the resulting stream. Each interface, template or wildcard filter means additional events that will match the query. The impact of include_interface_view and include_created_event_blob fields in the filters will also be accumulated. At least one cumulative filter MUST be specified. A template or an interface SHOULD NOT appear twice in the accumulative field. A wildcard filter SHOULD NOT be defined more than once in the accumulative field. Optional | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



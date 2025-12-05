# JsTopologyTransaction

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**update_id** | **String** | Assigned by the server. Useful for correlating logs. Must be a valid LedgerString (as described in ``value.proto``). Required | 
**events** | Option<[**Vec<models::TopologyEvent>**](TopologyEvent.md)> | A non-empty list of topology events. Required | [optional]
**offset** | **i64** | The absolute offset. The details of this field are described in ``community/ledger-api/README.md``. Required, it is a valid absolute offset (positive integer). | 
**synchronizer_id** | **String** | A valid synchronizer id. Identifies the synchronizer that synchronized the topology transaction. Required | 
**trace_context** | Option<[**models::TraceContext**](TraceContext.md)> |  | [optional]
**record_time** | **String** | The time at which the changes in the topology transaction become effective. There is a small delay between a topology transaction being sequenced and the changes it contains becoming effective. Topology transactions appear in order relative to a synchronizer based on their effective time rather than their sequencing time. Required | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



# TransactionHistoryResponseItem

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**transaction_type** | **String** | Describes the type of activity that occurred. Determines if the data for the transaction should be read from the `transfer`, `mint`, or `tap` property.  | 
**event_id** | **String** | The event id.  | 
**offset** | Option<**String**> | The ledger offset of the event. Note that this field may not be the same across nodes, and therefore should not be compared between SVs.  | [optional]
**date** | **String** | The effective date of the event.  | 
**domain_id** | **String** | The id of the domain through which this transaction was sequenced.  | 
**round** | Option<**i64**> | The round for which this transaction was registered.  | [optional]
**amulet_price** | Option<**String**> | The amulet price for the round at which this transfer was executed.  | [optional]
**transfer** | Option<[**models::Transfer**](Transfer.md)> |  | [optional]
**mint** | Option<[**models::AmuletAmount**](AmuletAmount.md)> |  | [optional]
**tap** | Option<[**models::AmuletAmount**](AmuletAmount.md)> |  | [optional]
**abort_transfer_instruction** | Option<[**models::AbortTransferInstruction**](AbortTransferInstruction.md)> |  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



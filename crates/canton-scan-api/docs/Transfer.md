# Transfer

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**provider** | **String** | The application provider.  | 
**sender** | [**models::SenderAmount**](SenderAmount.md) |  | 
**receivers** | [**Vec<models::ReceiverAmount>**](ReceiverAmount.md) | The amounts and fees per receiver.  | 
**balance_changes** | [**Vec<models::BalanceChange>**](BalanceChange.md) | Normalized balance changes per party caused by this transfer.  | 
**description** | Option<**String**> |  | [optional]
**transfer_instruction_receiver** | Option<**String**> |  | [optional]
**transfer_instruction_amount** | Option<**String**> |  | [optional]
**transfer_instruction_cid** | Option<**String**> |  | [optional]
**transfer_kind** | Option<**String**> |  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



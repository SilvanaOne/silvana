# TransferFactoryWithChoiceContext

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**factory_id** | **String** | The contract ID of the contract implementing the factory interface. | 
**transfer_kind** | **String** | The kind of transfer workflow that will be used: * `offer`: offer a transfer to the receiver and only transfer if they accept * `direct`: transfer directly to the receiver without asking them for approval.   Only chosen if the receiver has pre-approved direct transfers. * `self`: a self-transfer where the sender and receiver are the same party.   No approval is required, and the transfer is typically immediate.  | 
**choice_context** | [**models::ChoiceContext**](ChoiceContext.md) |  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



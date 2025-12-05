# TransferCommandFailedResponse

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**status** | **String** | The status of the transfer command. created:   The transfer command has been created and is waiting for automation to complete it. sent:   The transfer command has been completed and the transfer to the receiver has finished. failed:   The transfer command has failed permanently and nothing has been transferred. Refer to   failure_reason for details. A new transfer command can be created.  | 
**failure_kind** | **String** | The reason for the failure of the TransferCommand. failed:   Completing the transfer failed, check the reason for details. withdrawn:   The sender has withdrawn the TransferCommand before it could be completed. expired:   The expiry time on the TransferCommand was reached before it could be completed.  | 
**reason** | **String** | Human readable description of the failure  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



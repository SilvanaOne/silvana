# Completion1

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**command_id** | **String** | The ID of the succeeded or failed command. Must be a valid LedgerString (as described in ``value.proto``). Required | 
**status** | Option<[**models::Status**](Status.md)> |  | [optional]
**update_id** | **String** | The update_id of the transaction or reassignment that resulted from the command with command_id. Only set for successfully executed commands. Must be a valid LedgerString (as described in ``value.proto``). | 
**user_id** | **String** | The user-id that was used for the submission, as described in ``commands.proto``. Must be a valid UserIdString (as described in ``value.proto``). Optional for historic completions where this data is not available. | 
**act_as** | Option<**Vec<String>**> | The set of parties on whose behalf the commands were executed. Contains the ``act_as`` parties from ``commands.proto`` filtered to the requesting parties in CompletionStreamRequest. The order of the parties need not be the same as in the submission. Each element must be a valid PartyIdString (as described in ``value.proto``). Optional for historic completions where this data is not available. | [optional]
**submission_id** | **String** | The submission ID this completion refers to, as described in ``commands.proto``. Must be a valid LedgerString (as described in ``value.proto``). Optional | 
**deduplication_period** | [**models::DeduplicationPeriod1**](DeduplicationPeriod1.md) |  | 
**trace_context** | Option<[**models::TraceContext**](TraceContext.md)> |  | [optional]
**offset** | **i64** | May be used in a subsequent CompletionStreamRequest to resume the consumption of this stream at a later time. Required, must be a valid absolute offset (positive integer). | 
**synchronizer_time** | Option<[**models::SynchronizerTime**](SynchronizerTime.md)> |  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



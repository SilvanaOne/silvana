# JsExecuteSubmissionRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**prepared_transaction** | Option<**String**> | the prepared transaction Typically this is the value of the `prepared_transaction` field in `PrepareSubmissionResponse` obtained from calling `prepareSubmission`. | [optional]
**party_signatures** | Option<[**models::PartySignatures**](PartySignatures.md)> |  | [optional]
**deduplication_period** | [**models::DeduplicationPeriod2**](DeduplicationPeriod2.md) |  | 
**submission_id** | **String** | A unique identifier to distinguish completions for different submissions with the same change ID. Typically a random UUID. Applications are expected to use a different UUID for each retry of a submission with the same change ID. Must be a valid LedgerString (as described in ``value.proto``).  Required | 
**user_id** | **String** | See [PrepareSubmissionRequest.user_id] | 
**hashing_scheme_version** | **String** | The hashing scheme version used when building the hash | 
**min_ledger_time** | Option<[**models::MinLedgerTime**](MinLedgerTime.md)> |  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



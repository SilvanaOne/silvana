# JsPrepareSubmissionResponse

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**prepared_transaction** | Option<**String**> | The interpreted transaction, it represents the ledger changes necessary to execute the commands specified in the request. Clients MUST display the content of the transaction to the user for them to validate before signing the hash if the preparing participant is not trusted. | [optional]
**prepared_transaction_hash** | **String** | Hash of the transaction, this is what needs to be signed by the party to authorize the transaction. Only provided for convenience, clients MUST recompute the hash from the raw transaction if the preparing participant is not trusted. May be removed in future versions | 
**hashing_scheme_version** | **String** | The hashing scheme version used when building the hash | 
**hashing_details** | Option<**String**> | Optional additional details on how the transaction was encoded and hashed. Only set if verbose_hashing = true in the request Note that there are no guarantees on the stability of the format or content of this field. Its content should NOT be parsed and should only be used for troubleshooting purposes. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



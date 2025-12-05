# CompletionStreamRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**user_id** | **String** | Only completions of commands submitted with the same user_id will be visible in the stream. Must be a valid UserIdString (as described in ``value.proto``). Required unless authentication is used with a user token. In that case, the token's user-id will be used for the request's user_id. | 
**parties** | Option<**Vec<String>**> | Non-empty list of parties whose data should be included. The stream shows only completions of commands for which at least one of the ``act_as`` parties is in the given set of parties. Must be a valid PartyIdString (as described in ``value.proto``). Required | [optional]
**begin_exclusive** | **i64** | This optional field indicates the minimum offset for completions. This can be used to resume an earlier completion stream. If not set the ledger uses the ledger begin offset instead. If specified, it must be a valid absolute offset (positive integer) or zero (ledger begin offset). If the ledger has been pruned, this parameter must be specified and greater than the pruning offset. | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



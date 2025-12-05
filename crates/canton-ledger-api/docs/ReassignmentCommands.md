# ReassignmentCommands

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**workflow_id** | **String** | Identifier of the on-ledger workflow that this command is a part of. Must be a valid LedgerString (as described in ``value.proto``). Optional | 
**user_id** | **String** | Uniquely identifies the participant user that issued the command. Must be a valid UserIdString (as described in ``value.proto``). Required unless authentication is used with a user token. In that case, the token's user-id will be used for the request's user_id. | 
**command_id** | **String** | Uniquely identifies the command. The triple (user_id, submitter, command_id) constitutes the change ID for the intended ledger change. The change ID can be used for matching the intended ledger changes with all their completions. Must be a valid LedgerString (as described in ``value.proto``). Required | 
**submitter** | **String** | Party on whose behalf the command should be executed. If ledger API authorization is enabled, then the authorization metadata must authorize the sender of the request to act on behalf of the given party. Must be a valid PartyIdString (as described in ``value.proto``). Required | 
**submission_id** | **String** | A unique identifier to distinguish completions for different submissions with the same change ID. Typically a random UUID. Applications are expected to use a different UUID for each retry of a submission with the same change ID. Must be a valid LedgerString (as described in ``value.proto``).  If omitted, the participant or the committer may set a value of their choice. Optional | 
**commands** | Option<[**Vec<models::ReassignmentCommand>**](ReassignmentCommand.md)> | Individual elements of this reassignment. Must be non-empty. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



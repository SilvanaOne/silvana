# JsPrepareSubmissionRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**user_id** | **String** | Uniquely identifies the participant user that prepares the transaction. Must be a valid UserIdString (as described in ``value.proto``). Required unless authentication is used with a user token. In that case, the token's user-id will be used for the request's user_id. | 
**command_id** | **String** | Uniquely identifies the command. The triple (user_id, act_as, command_id) constitutes the change ID for the intended ledger change, where act_as is interpreted as a set of party names. The change ID can be used for matching the intended ledger changes with all their completions. Must be a valid LedgerString (as described in ``value.proto``). Required | 
**commands** | Option<[**Vec<models::Command>**](Command.md)> | Individual elements of this atomic command. Must be non-empty. Required | [optional]
**min_ledger_time** | Option<[**models::MinLedgerTime**](MinLedgerTime.md)> |  | [optional]
**act_as** | Option<**Vec<String>**> | Set of parties on whose behalf the command should be executed, if submitted. If ledger API authorization is enabled, then the authorization metadata must authorize the sender of the request to **read** (not act) on behalf of each of the given parties. This is because this RPC merely prepares a transaction and does not execute it. Therefore read authorization is sufficient even for actAs parties. Note: This may change, and more specific authorization scope may be introduced in the future. Each element must be a valid PartyIdString (as described in ``value.proto``). Required, must be non-empty. | [optional]
**read_as** | Option<**Vec<String>**> | Set of parties on whose behalf (in addition to all parties listed in ``act_as``) contracts can be retrieved. This affects Daml operations such as ``fetch``, ``fetchByKey``, ``lookupByKey``, ``exercise``, and ``exerciseByKey``. Note: A command can only use contracts that are visible to at least one of the parties in ``act_as`` or ``read_as``. This visibility check is independent from the Daml authorization rules for fetch operations. If ledger API authorization is enabled, then the authorization metadata must authorize the sender of the request to read contract data on behalf of each of the given parties. Optional | [optional]
**disclosed_contracts** | Option<[**Vec<models::DisclosedContract>**](DisclosedContract.md)> | Additional contracts used to resolve contract & contract key lookups. Optional | [optional]
**synchronizer_id** | **String** | Must be a valid synchronizer id Required | 
**package_id_selection_preference** | Option<**Vec<String>**> | The package-id selection preference of the client for resolving package names and interface instances in command submission and interpretation | [optional]
**verbose_hashing** | **bool** | When true, the response will contain additional details on how the transaction was encoded and hashed This can be useful for troubleshooting of hash mismatches. Should only be used for debugging. | 
**max_record_time** | Option<**String**> | Maximum timestamp at which the transaction can be recorded onto the ledger via the synchronizer specified in the `PrepareSubmissionResponse`. If submitted after it will be rejected even if otherwise valid, in which case it needs to be prepared and signed again with a new valid max_record_time. Use this to limit the time-to-life of a prepared transaction, which is useful to know when it can definitely not be accepted anymore and resorting to preparing another transaction for the same intent is safe again. Optional | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



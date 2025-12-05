# ArchivedEvent

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**offset** | **i64** | The offset of origin. Offsets are managed by the participant nodes. Transactions can thus NOT be assumed to have the same offsets on different participant nodes. Required, it is a valid absolute offset (positive integer) | 
**node_id** | **i32** | The position of this event in the originating transaction or reassignment. Node IDs are not necessarily equal across participants, as these may see different projections/parts of transactions. Required, must be valid node ID (non-negative integer) | 
**contract_id** | **String** | The ID of the archived contract. Must be a valid LedgerString (as described in ``value.proto``). Required | 
**template_id** | **String** | The template of the archived contract. The identifier uses the package-id reference format.  Required | 
**witness_parties** | Option<**Vec<String>**> | The parties that are notified of this event. For an ``ArchivedEvent``, these are the intersection of the stakeholders of the contract in question and the parties specified in the ``TransactionFilter``. The stakeholders are the union of the signatories and the observers of the contract. Each one of its elements must be a valid PartyIdString (as described in ``value.proto``). Required | [optional]
**package_name** | **String** | The package name of the contract. Required | 
**implemented_interfaces** | Option<**Vec<String>**> | The interfaces implemented by the target template that have been matched from the interface filter query. Populated only in case interface filters with include_interface_view set.  If defined, the identifier uses the package-id reference format.  Optional | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



# CreatedEvent

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**offset** | **i64** | The offset of origin, which has contextual meaning, please see description at messages that include a CreatedEvent. Offsets are managed by the participant nodes. Transactions can thus NOT be assumed to have the same offsets on different participant nodes. Required, it is a valid absolute offset (positive integer) | 
**node_id** | **i32** | The position of this event in the originating transaction or reassignment. The origin has contextual meaning, please see description at messages that include a CreatedEvent. Node IDs are not necessarily equal across participants, as these may see different projections/parts of transactions. Required, must be valid node ID (non-negative integer) | 
**contract_id** | **String** | The ID of the created contract. Must be a valid LedgerString (as described in ``value.proto``). Required | 
**template_id** | **String** | The template of the created contract. The identifier uses the package-id reference format.  Required | 
**contract_key** | Option<[**serde_json::Value**](.md)> | The key of the created contract. This will be set if and only if ``create_arguments`` is set and ``template_id`` defines a contract key. Optional | [optional]
**create_argument** | Option<[**serde_json::Value**](.md)> |  | [optional]
**created_event_blob** | **String** | Opaque representation of contract create event payload intended for forwarding to an API server as a contract disclosed as part of a command submission. Optional | 
**interface_views** | Option<[**Vec<models::JsInterfaceView>**](JsInterfaceView.md)> | Interface views specified in the transaction filter. Includes an ``InterfaceView`` for each interface for which there is a ``InterfaceFilter`` with  - its party in the ``witness_parties`` of this event, - and which is implemented by the template of this event, - and which has ``include_interface_view`` set.  Optional | [optional]
**witness_parties** | Option<**Vec<String>**> | The parties that are notified of this event. When a ``CreatedEvent`` is returned as part of a transaction tree or ledger-effects transaction, this will include all the parties specified in the ``TransactionFilter`` that are informees of the event. If served as part of a ACS delta transaction those will be limited to all parties specified in the ``TransactionFilter`` that are stakeholders of the contract (i.e. either signatories or observers). If the ``CreatedEvent`` is returned as part of an AssignedEvent, ActiveContract or IncompleteUnassigned (so the event is related to an assignment or unassignment): this will include all parties of the ``TransactionFilter`` that are stakeholders of the contract.  The behavior of reading create events visible to parties not hosted on the participant node serving the Ledger API is undefined. Concretely, there is neither a guarantee that the participant node will serve all their create events on the ACS stream, nor is there a guarantee that matching archive events are delivered for such create events.  For most clients this is not a problem, as they only read events for parties that are hosted on the participant node. If you need to read events for parties that may not be hosted at all times on the participant node, subscribe to the ``TopologyEvent``s for that party by setting a corresponding ``UpdateFormat``.  Using these events, query the ACS as-of an offset where the party is hosted on the participant node, and ignore create events at offsets where the party is not hosted on the participant node. Required | [optional]
**signatories** | Option<**Vec<String>**> | The signatories for this contract as specified by the template. Required | [optional]
**observers** | Option<**Vec<String>**> | The observers for this contract as specified explicitly by the template or implicitly as choice controllers. This field never contains parties that are signatories. Required | [optional]
**created_at** | **String** | Ledger effective time of the transaction that created the contract. Required | 
**package_name** | **String** | The package name of the created contract. Required | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



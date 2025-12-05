# InterfaceFilter1

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**interface_id** | Option<**String**> | The interface that a matching contract must implement. The ``interface_id`` needs to be valid: corresponding interface should be defined in one of the available packages at the time of the query. Both package-name and package-id reference formats for the identifier are supported. Note: The package-id reference identifier format is deprecated. We plan to end support for this format in version 3.4.  Required | [optional]
**include_interface_view** | **bool** | Whether to include the interface view on the contract in the returned ``CreatedEvent``. Use this to access contract data in a uniform manner in your API client. Optional | 
**include_created_event_blob** | **bool** | Whether to include a ``created_event_blob`` in the returned ``CreatedEvent``. Use this to access the contract create event payload in your API client for submitting it as a disclosed contract with future commands. Optional | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)



# UserManagementFeature

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**supported** | **bool** | Whether the Ledger API server provides the user management service. | 
**max_rights_per_user** | **i32** | The maximum number of rights that can be assigned to a single user. Servers MUST support at least 100 rights per user. A value of 0 means that the server enforces no rights per user limit. | 
**max_users_page_size** | **i32** | The maximum number of users the server can return in a single response (page). Servers MUST support at least a 100 users per page. A value of 0 means that the server enforces no page size limit. | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


